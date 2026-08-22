# Ground-truth capture: `spike_ei_i_lt_x.txt`

The Phase 1 spike's exit criterion (`docs/DESIGN.md` §8; plan at
`.claude/plans/fluttering-foraging-spindle.md`) needs a real `walnut-java` output
to compare the Rust pipeline against. This is a one-time, manually-run capture —
not a live per-test JVM shellout (that's Tier-3/Phase-4 scope) — committed as a
fixture.

## Query

```
eval spike "?msd_2 Ei i<x";
```

## How it was captured (reproducible)

From a built `walnut-java` checkout (`./mvnw -q clean package -DskipTests -Pfat-jar`,
producing `target/Walnut-all.jar`):

```bash
cd ~/dev/walnut-java
cat > "Command Files/spike_capture.txt" <<'EOF'
eval spike "?msd_2 Ei i<x";
EOF
java -jar target/Walnut-all.jar spike_capture.txt < /dev/null
```

(`< /dev/null` avoids the interactive REPL prompt Walnut drops into after
processing the command file — it otherwise hangs waiting on stdin.)

Output lands at `Session/<timestamp>/Result/spike.txt` (identical to
`Session/<timestamp>/Automata Library/spike.txt`) — copied verbatim into
`fixtures/spike_ei_i_lt_x.txt` here. The command file and session directory were
deleted from the `walnut-java` checkout afterward (not part of that repo's
tracked history).

## Result (for reference — the fixture file is authoritative)

```
msd_2

0 0
0 -> 0
1 -> 1

1 1
0 -> 1
1 -> 1
```

The classic 2-state "contains a 1" DFA: state 0 (start, non-accepting) self-loops
on `0`, moves to state 1 on `1`; state 1 (accepting) self-loops on everything.
Exactly `x ≠ 0` read msd-first, matching the hand-derivation already pinned by
`wr-logic`'s own `exists_i_less_than_x_is_x_nonzero` unit test — this fixture is
what lets `tests/spike_ei_i_lt_x.rs` confirm the same result independently, via
the real oracle rather than a second hand-derivation.

---

# Ground-truth capture: the `reg` corpus (`fixtures/reg/*.txt`)

Phase 3a U8 (`wr_core::regex`, the hand-rolled Brics-dialect engine) is checked against
57 real `reg` outputs by `tests/reg_brics_regex.rs` (60 captured originally; `r19`/`r50`/
`r58` removed 2026-08-20 once WB-024's fix landed, since none of the three still builds an
automaton — see that file's own module docs, and the `java_bugfix_wb024_wb025.rs` entry
below for where their coverage moved to, not just disappeared). Same one-time-capture
discipline as above — no live JVM shellout from the test.

## How they were captured (reproducible)

Write one command file holding every case, in the same order as `corpus()` in
`tests/reg_brics_regex.rs` (each line is `reg r<NN> <alphabets> "<regex>";`, and `r<NN>`
is the fixture's file name), then:

```bash
cd ~/dev/walnut-java
cp /path/to/that/file "Command Files/u8reg.txt"
java -jar target/Walnut-all.jar u8reg.txt < /dev/null
cp "Session/<timestamp>/Automata Library/"*.txt \
   ~/dev/walnut-rs/tests/differential/fixtures/reg/
```

Keep the command file's regexes and the Rust `corpus()` table in sync by hand: the
fixture name is the only link between them, and a mismatch would silently compare the
Rust result for one regex against Walnut's output for a different one. (The Rust side
re-derives the automaton from the `(alphabets, regex)` pair in the table, so a drifted
table shows up as a language-equivalence failure, not as a silent pass — but it would be
diagnosed as an engine bug rather than a bookkeeping one.)

The four cases that make real Walnut **throw** produce no file; their expected messages
are pinned inline by `reg_parse_errors_match_real_walnut_messages` instead, from the same
run's stdout:

```
reg r?? {0,1} "0|";        java.lang.IllegalArgumentException: expected ')' at position 10
reg r?? {0,1} "<abc>";     java.lang.IllegalArgumentException: 'abc' not found
reg r?? {0,1} "<1-5>";     java.lang.IllegalArgumentException: interval syntax error at position 5
reg r?? {0,1} "0{2,3}";    java.lang.IllegalArgumentException: integer expected at position 3
```

## The one constructor with no CLI command behind it

`AutomatonDFA(String, List<Integer>, NumberSystem)` (Java's `convertFromBrics` path) has
no `Prover` command, so its ground truth came from a throwaway Java driver compiled
against the same jar:

```bash
/opt/homebrew/opt/openjdk@17/bin/javac -cp ~/dev/walnut-java/target/Walnut-all.jar -d /tmp/u8 U8Probe.java
/opt/homebrew/opt/openjdk@17/bin/java  -cp ~/dev/walnut-java/target/Walnut-all.jar:/tmp/u8 U8Probe
```

where `U8Probe.main` calls `new AutomatonDFA(regex, alphabet, null)` and prints
`Q`/`q0`/acceptance/transitions per state. Its results are pinned as state counts and
explicit languages in `crates/wr-core/src/regex/tests.rs`'s
`from_regex_over_alphabet_*` tests (the driver itself is not committed — it is 30 lines
and fully described by that recipe).

---

# Ground-truth capture: U16's `reg` (named number systems) and `alphabet` fixtures

Phase 3a U16 (`wr_cli::reg`/`wr_cli::alphabet`, the CLI-layer wiring around U8's regex
engine and `Automaton.setAlphabet`) is checked by
`tests/reg_and_alphabet_commands.rs`. Most of its `reg` coverage reuses U8's own 60-case
corpus (`fixtures/reg/r*.txt`) unchanged, re-driven through the STRING alphabet-
declaration layer U16 adds — no new capture needed for that part (see that test file's
own module docs for why). Two things are genuinely new:

## `reg` with named number systems (`fixtures/reg/u16_*.txt`)

None of U8's 60 fixtures declare a `reg` alphabet by number-system name (`msd_3`, …) —
every one uses a literal `{…}` set. Captured with:

```bash
cd ~/dev/walnut-java
cat > "Command Files/u16capture.txt" <<'EOF'
reg u16r01 msd_3 "0*1";
reg u16r02 msd_2 lsd_2 "[0,0][1,1]*";
reg u16r03 {0,1,2} "1*";
EOF
java -jar target/Walnut-all.jar u16capture.txt < /dev/null
cp "Session/<timestamp>/Automata Library/u16r01.txt" ~/dev/walnut-rs/tests/differential/fixtures/reg/u16_msd3.txt
cp "Session/<timestamp>/Automata Library/u16r02.txt" ~/dev/walnut-rs/tests/differential/fixtures/reg/u16_mixed_ns.txt
cp "Session/<timestamp>/Automata Library/u16r03.txt" ~/dev/walnut-rs/tests/differential/fixtures/reg/u16_set.txt
```

(`u16r03`'s `{0,1,2}` case is redundant with U8's own corpus in spirit — kept anyway as a
belt-and-suspenders sanity check alongside the two number-system cases captured in the
same run.)

## `alphabet` (`fixtures/alphabet/*.txt`)

No prior unit captured the `alphabet` command at all. Two source/result pairs, each
`reg`'d fresh and then run through `alphabet` in the same command file so the source
automaton is also pinned as ground truth (`baseB.txt`/`baseC.txt`):

```bash
cd ~/dev/walnut-java
cat > "Command Files/u16capture2.txt" <<'EOF'
reg baseB msd_2 msd_2 "[0,0][1,1]*";
alphabet baseB_asSet {0,1} {0,1} $baseB;
EOF
cat > "Command Files/u16capture3.txt" <<'EOF'
reg baseC {0,1,2,3} "[0-3]";
alphabet baseC_restricted msd_2 $baseC;
EOF
java -jar target/Walnut-all.jar u16capture2.txt < /dev/null
java -jar target/Walnut-all.jar u16capture3.txt < /dev/null
cp "Session/<ts2>/Automata Library/baseB.txt"            ~/dev/walnut-rs/tests/differential/fixtures/alphabet/
cp "Session/<ts2>/Automata Library/baseB_asSet.txt"      ~/dev/walnut-rs/tests/differential/fixtures/alphabet/
cp "Session/<ts3>/Automata Library/baseC.txt"            ~/dev/walnut-rs/tests/differential/fixtures/alphabet/
cp "Session/<ts3>/Automata Library/baseC_restricted.txt" ~/dev/walnut-rs/tests/differential/fixtures/alphabet/
```

`baseC`'s regex (`"[0-3]"`, i.e. "any single symbol 0-3" via the same char-range-over-
encoded-digits idiom U8's `r08`/`r15`/`r31` already established) was chosen specifically
so every one of the four declared digits has a real outgoing transition from the start
state — `alphabet baseC_restricted msd_2 $baseC` then genuinely PRUNES the digit-2/3
transitions (confirmed by inspecting both captured files), rather than only rewriting
the header, which a less deliberately-chosen source automaton could have masked. `baseB`
exercises the opposite direction: a named-number-system automaton converted to an
equivalent literal-set alphabet (same digits, `NS` cleared), the `None`-NS/`all_reps`-
clearing path `set_alphabet` shares with `reg`.

Only `isDFAO = false` (`$`-prefixed old-name syntax, per `Alphabet.java`'s inverted-
looking `!"$".equals(...)` flag — see `RESUME-HERE.md`/`crate::alphabet`'s module docs)
was captured; the `isDFAO = true` (word-automaton) path is covered by
`wr_core::word_automaton`'s own existing unit tests
(`minimize_self_with_output_mutates_in_place`, etc.) rather than a fresh empirical
capture — flagged here as a real, deliberate scope cut for whoever reviews this unit,
not an oversight: setting up a genuine word (DFAO) automaton via the command-line surface
needs `morphism`/`image`/`combine`, none of which are ported yet.

Command files and session directories were deleted from the `walnut-java` checkout
afterward, per this file's established practice.

---

# Ground-truth capture: `fixtures/u11/*.txt`

Phase 3a U11 (`wr_logic::eval`, the postfix-token executor + final `Predicate`
assembly) is spot-checked against real `walnut-java` `eval` output by
`tests/u11_eval_composition.rs`. Full recipe (and the fixture-less closed-formula case)
is in that test file's own module docs; summarized here for consistency with this
document's other entries:

```bash
cd ~/dev/walnut-java
cat > "Command Files/u11_capture.txt" <<'EOF'
eval u11check "?msd_2 x>=2 & x<5";
EOF
java -jar target/Walnut-all.jar u11_capture.txt < /dev/null
cp "Session/<timestamp>/Automata Library/u11check.txt" \
   ~/dev/walnut-rs/tests/differential/fixtures/u11/boolean_relational.txt

cat > "Command Files/u11_capture3.txt" <<'EOF'
eval u11xyz "?msd_2 x + y = z";
def zphi "?msd_2 a < b";
EOF
java -jar target/Walnut-all.jar u11_capture3.txt < /dev/null
cp "Session/<timestamp>/Automata Library/u11xyz.txt" \
   ~/dev/walnut-rs/tests/differential/fixtures/u11/addition_three_track.txt
cp "Session/<timestamp>/Automata Library/zphi.txt" \
   ~/dev/walnut-rs/tests/differential/fixtures/u11/zphi_a_lt_b.txt
```

The companion closed-formula case (`eval u11closed "?msd_2 Ex (x < 5 & x >= 2)";`)
prints `____` then `TRUE` on stdout — no `.txt` fixture, since a trivial automaton has
no meaningful body; `tests/u11_eval_composition.rs` checks the printed verdict directly
against `Automaton::fa::is_true_automaton()`.

## Every fixture here is in ALPHABETICAL track order — normalize before comparing

`AutomatonWriter.writeToTxtFormat` calls `automaton.canonize()`
(`Automata/Writer/AutomatonWriter.java:52`) → `sortLabel()` (`Automata/Automaton.java:328`,
`:348-379`), so a captured multi-track `.txt` always lists its tracks sorted by label. The
Rust pipeline does **not** — `?msd_2 x + y = z` comes back labeled `["z", "x", "y"]`. Call
`Automaton::sort_label()` on the port's result before handing it to
`wr_core::equiv::automaton_language_equivalent`, which by design does **not** detect a
label permutation with matching per-position alphabets (U8's documented limitation) and
will silently return a wrong verdict instead of an error. `fixtures/u11/addition_three_track.txt`
exists partly to pin exactly that: without the sort, its test reports `Ok(false)`.

---

# Ground-truth capture: `fixtures/lsd/*.txt`

Phase 3b's L1 (wiring `AutomatonLogicalOps.fixTrailingZerosProblem` into
`wr_core::quantify`'s lsd branch, which Phase 2 had left as a hard
`QuantifyError::UnsupportedLsdFixup`) is checked against real `walnut-java` `eval` output
by `tests/lsd_numeration.rs`. Before L1 the port had **zero** positive `lsd_k` end-to-end
coverage — every existing test pinned the rejection — so these are the fixtures that
close that gap. Full rationale (including why each of the seven cases fails for a
different reason if the fixup is wired up wrongly) is in that test file's own module
docs; summarized here for consistency with this document's other entries:

```bash
cd ~/dev/walnut-java
cat > "Command Files/lsd_capture.txt" <<'EOF'
eval lsdge2 "?lsd_2 x >= 2";
eval lsdquant "?lsd_2 Ex (x < 5 & x >= 2 & y = x)";
eval lsdmult "?lsd_2 y = 3*x";
eval lsdclosed "?lsd_2 Ax (x >= 5)";
eval lsdclosedtrue "?lsd_2 Ax Ey (y > x)";
eval lsd3addcmp "?lsd_3 x + y = z & z < 4";
EOF
java -jar target/Walnut-all.jar lsd_capture.txt < /dev/null
S="Session/<timestamp>/Automata Library"
cp "$S/lsdge2.txt"     ~/dev/walnut-rs/tests/differential/fixtures/lsd/ge_two.txt
cp "$S/lsdquant.txt"   ~/dev/walnut-rs/tests/differential/fixtures/lsd/exists_quantified.txt
cp "$S/lsdmult.txt"    ~/dev/walnut-rs/tests/differential/fixtures/lsd/mult_two_track.txt
cp "$S/lsd3addcmp.txt" ~/dev/walnut-rs/tests/differential/fixtures/lsd/base3_addition_and_compare.txt

# A second run, for the `def`-then-reuse case (`def` must land in `Automata Library/`
# before the query that references it).
cat > "Command Files/lsd_capture2.txt" <<'EOF'
def lsdge2d "?lsd_2 x >= 2";
eval lsdusedef "?lsd_2 $lsdge2d(y) & y < 5";
EOF
java -jar target/Walnut-all.jar lsd_capture2.txt < /dev/null
cp "Session/<timestamp2>/Automata Library/lsdusedef.txt" \
   ~/dev/walnut-rs/tests/differential/fixtures/lsd/def_then_reuse.txt
```

The two closed-formula cases produce no `.txt` fixture: `lsdclosed` prints `____` then
`FALSE`, `lsdclosedtrue` prints `____` then `TRUE`, and `tests/lsd_numeration.rs` checks
those printed verdicts directly against `Automaton::fa::is_true_automaton()` — same
convention as the `u11closed` entry above. The command file and
`Session/<timestamp>/` directory were deleted from the `walnut-java` checkout afterward.

The alphabetical-track-order note above applies here too (`lsd3addcmp` is three-track),
as does the totalize-before-comparing step: real Walnut's automaton for a free-variable
predicate is partial.

---

# Ground-truth capture: `fixtures/lsd_custom_base/*.txt`

`docs/BACKLOG-LSD-INFINITE-LOGGING-DISPATCH.md` item 2 (custom-base `lsd` verification) is
checked against real `walnut-java` `eval`/`def` output by `tests/lsd_custom_base.rs`. Prior
`lsd` coverage was plain `lsd_k` only in THIS differential suite (`fixtures/lsd/`, the L1
entry above) — these fixtures are the first over a real CUSTOM base's `lsd` direction *in
this suite*. Java's own gated-slow Tier-1 golden corpus already covers `∃`/open `∀` over
`lsd_fib` (e.g. fixtures 65/110-115/135, `phase0-artifacts/test-manifest.json`), passing;
what's genuinely new here is fast-tier presence plus `I` and `def`-then-`$token` reuse over
`lsd_fib`, neither of which any existing fixture (gated-slow or otherwise) covers. `lsd_fib`'s
adder exists only because `NumberSystem`'s opposite-direction-complement fallback
language-reverses `Custom Bases/msd_fib_addition.txt` (walnut-java ships **no**
`lsd_fib*.txt`, so this is the only way `?lsd_fib` resolves on either engine). Full
rationale, and the mutation matrix saying which case catches what (and which of those a
gated-slow golden fixture would also catch), are in that test file's own module docs.

```bash
cd ~/dev/walnut-java     # built with ./mvnw -q clean package -DskipTests -Pfat-jar
cat > "Command Files/lsdfib_capture.txt" <<'CMD'
eval lfge2 "?lsd_fib x >= 2";
eval lfquant "?lsd_fib Ex (x < 5 & x >= 2 & y = x)";
eval lfclosed "?lsd_fib Ax (x >= 5)";
eval lfclosedtrue "?lsd_fib Ax Ey (y > x)";
eval lfinffalse "?lsd_fib Ix x < 5";
eval lfinftrue "?lsd_fib Ix x >= 5";
eval lfadd "?lsd_fib x + y = z & z < 4";
eval lfforall "?lsd_fib Ay (y < 3 => y < x)";
CMD
java -jar target/Walnut-all.jar lsdfib_capture.txt < /dev/null
S="Session/<timestamp>/Automata Library"
cp "$S/lfge2.txt"    .../fixtures/lsd_custom_base/ge_two.txt
cp "$S/lfquant.txt"  .../fixtures/lsd_custom_base/exists_quantified.txt
cp "$S/lfadd.txt"    .../fixtures/lsd_custom_base/addition_three_track.txt
cp "$S/lfforall.txt" .../fixtures/lsd_custom_base/forall_open.txt

# A second run, for the `def`-then-reuse case.
cat > "Command Files/lsdfib_capture2.txt" <<'CMD'
def lfge2d "?lsd_fib x >= 2";
eval lfusedef "?lsd_fib $lfge2d(y) & y < 5";
CMD
java -jar target/Walnut-all.jar lsdfib_capture2.txt < /dev/null
cp "Session/<timestamp2>/Automata Library/lfusedef.txt" \
   .../fixtures/lsd_custom_base/def_then_reuse.txt
```

The four closed cases (`lfclosed`/`lfclosedtrue`/`lfinffalse`/`lfinftrue`) produce no `.txt`
fixture — they print `____` then `FALSE`/`TRUE`/`FALSE`/`TRUE` on stdout, and the test checks
those verdicts directly against `Automaton::fa::is_true_automaton()`, the same convention the
`u11closed` and `lsdclosed` entries above use. The command files and `Session/<timestamp>/`
directories were deleted from the `walnut-java` checkout afterward.

The alphabetical-track-order and `totalize(0)` notes from the `fixtures/lsd/` entry apply
here too (`lfadd` is three-track). One additional step is specific to this directory: every
fixture's header says `lsd_fib`, so it must be read with
`wr_io::reader::read_automaton_txt_with_custom_bases` against a directory holding
`msd_fib.txt`/`msd_fib_addition.txt` — plain `read_automaton_txt` rejects a custom-base
header with `ReadError::UnsupportedNumeration`.

---

# Ground-truth capture: `fixtures/cas_export/def_freevars_ok.*`

CAS matrix export (`docs/CAS-EXPORT-DISPATCH.md`, `.claude/plans/amber-transcribing-ledger.md`,
`wr_io::matrix_writer`) is checked by `tests/phase3a_checkpoint.rs`'s
`def_style_free_variable_list_passes_validation_and_writes_real_matrix_files`. Before this
capture, that test only checked non-empty content + a substring (`m.contains("M_x_y_")`) — an
adversarial review of the CAS-export diff found this survives a wrong matrix order, a wrong
fix-up representative, or wrong separators/braces in any of the four formats, and pointed out
the fast tier had NO byte-exact end-to-end pin of the `eval`/`def` → matrix-file pipeline (the
Tier-1 golden corpus's own byte-exact coverage of the same fixtures, 374-379/383, is
`#[ignore]`d, gated-slow). This capture closes that gap for the fast tier.

## Query

```
eval def_freevars_ok x y "?msd_2 x < 5 & y = x";
```

Chosen to match the pre-existing Rust test's call exactly (`?msd_2 x < 5 & y = x`, name
`def_freevars_ok`, free variables `x y`) — both names are real track labels on a non-trivial
result, so `AutomatonMatrixWriter.writeMatrix`'s validation succeeds and all four CAS files are
produced.

## How it was captured (reproducible)

```bash
cd ~/dev/walnut-java   # already built: target/Walnut-all.jar
cat > "Command Files/casexport_capture.txt" <<'EOF'
eval def_freevars_ok x y "?msd_2 x < 5 & y = x";
EOF
java -jar target/Walnut-all.jar casexport_capture.txt < /dev/null
S="Session/<timestamp>/Result"
cp "$S/def_freevars_ok.mpl" "$S/def_freevars_ok.m" "$S/def_freevars_ok.wl" "$S/def_freevars_ok.sage" \
   ~/dev/walnut-rs/tests/differential/fixtures/cas_export/
```

Note this capture reads from `Session/<timestamp>/Result/`, not `.../Automata Library/` like
every other entry in this file — matrix files are written only to `Result/`
(`AutomatonMatrixWriter`/`EvalDef.writeMatrices`), never promoted to the library. The command
file and session directory were deleted from the `walnut-java` checkout afterward, per this
file's established practice.

The test compares `TestCase::matrix_output()`'s four strings against these four files,
trimmed, in `wr_io::matrix_writer::EMITTERS` order (Maple/MATLAB/Mathematica/Sage) — the same
order `tests/golden`'s `MATRIX_EXTENSIONS` uses and the order this project's plan flagged as
load-bearing for `matrix_output[i]` indexing.

---

# Ground-truth capture: the Ostrowski (`ost`) corpus (`fixtures/ostrowski/*.txt`)

Used by `tests/ostrowski.rs` and by two `crates/wr-cli/src/prover.rs` unit tests. Nine
files: the six `Custom Bases/` automata three `ost` commands write, plus the three
`Automata Library/` results of the follow-up queries over the bases they created.

## Commands

```
ost o [1 2] [3];
ost rotsingle [] [1];
ost numsys2 [0 3 1] [1 2];
eval ostq1 "?msd_o Ex x+x=y";
eval ostq2 "?msd_rotsingle Ax (x<3) => (x+1>x)";
eval ostq3 "?msd_numsys2 Ex,y x+y=z & x=y";
```

The three `ost` invocations were chosen to cover the constructor's three distinct
pre-period shapes — `[1, 2]` (the multi-digit `preperiod[0] == 1` rotation,
`Ostrowski.java:105-107`), `[]` (copy-filled from the period, then the *single*-digit
rotation at `:109-111`), and `[3, 1]` (no rotation; golden fixture 625's own arguments).
Neither rotation branch is reachable from any fixture in the golden corpus.

## How it was captured (reproducible)

```bash
cd ~/dev/walnut-java   # already built: target/Walnut-all.jar
cat > "Command Files/ost_capture.txt" <<'EOF2'
ost o [1 2] [3];
ost rotsingle [] [1];
ost numsys2 [0 3 1] [1 2];
eval ostq1 "?msd_o Ex x+x=y";
eval ostq2 "?msd_rotsingle Ax (x<3) => (x+1>x)";
eval ostq3 "?msd_numsys2 Ex,y x+y=z & x=y";
EOF2
java -jar target/Walnut-all.jar ost_capture.txt < /dev/null
S="Session/<timestamp>"
cp "$S/Custom Bases/"*.txt          ~/dev/walnut-rs/tests/differential/fixtures/ostrowski/
cp "$S/Result/ostq1.txt" "$S/Result/ostq2.txt" "$S/Result/ostq3.txt" \
                                    ~/dev/walnut-rs/tests/differential/fixtures/ostrowski/
```

The command file and session directory were deleted from the `walnut-java` checkout
afterward, per this file's established practice. **The capture was run twice,
independently, and all nine files came out byte-identical both times** — `ost`'s output
is deterministic, which is what lets the tests compare the two `Custom Bases/` files
byte-for-byte rather than only semantically. That byte-level comparison is deliberate:
`ost`'s entire observable output is the two files it writes, so a re-canonicalized-but-
language-equivalent automaton would be a genuine divergence that `wr_core::equiv` could
never see. The three query results are compared by `wr_core::equiv` semantic equivalence,
per `CLAUDE.md`'s Prime Directive.

`ostq2.txt` is the literal text `true` (a closed formula collapsing to the TRUE
automaton); real Walnut also printed `TRUE` on stdout for it.

---

# Ground-truth capture: negative-base numeration (`fixtures/negative_base/*.txt`)

Captured 2026-08-20 for `tests/negative_base.rs`, the Tier-3 half of
`docs/NEGATIVE-BASE-SPLIT-DISPATCH.md`'s Layer A (negative-base numeration:
`msd_neg_k` / `lsd_neg_k` / `msd_neg_fib` / `lsd_neg_fib`).

The full command file, the per-fixture `cp` list, and the reasoning for each query are in
`tests/negative_base.rs`'s own module documentation — kept there rather than duplicated
here because the mutation matrix that justifies the query choice lives beside it. In
outline:

```bash
cd ~/dev/walnut-java     # built with ./mvnw -q clean package -DskipTests -Pfat-jar
cat > "Command Files/negcap.txt" <<'EOF'
eval nblt          "?msd_neg_2 x < y";
eval nbltlsd       "?lsd_neg_2 x < y";
eval nbadd         "?msd_neg_2 x + y = z";
eval nbconst       "?msd_neg_2 x = _5";
eval nbquant       "?msd_neg_3 Ex (x + x = y & y < 5)";
eval nbdiv         "?msd_neg_2 y = x / _3";
eval nbmul         "?msd_neg_2 y = _2 * x";
eval nbfiblsd      "?lsd_neg_fib x >= 2";
eval nbfibmsd      "?msd_neg_fib Ex (x < 5 & y = x)";
eval nbclosed      "?msd_neg_2 Ax Ey (y > x)";
eval nbclosedfalse "?msd_neg_2 Ax (x >= 0)";
EOF
java -jar target/Walnut-all.jar negcap.txt < /dev/null
```

Nine automata were copied out of `Session/<timestamp>/Automata Library/`; the two closed
formulae print `TRUE`/`FALSE` on stdout and need no fixture file. The three
`Custom Bases/msd_neg_fib{,_addition,_less_than}.txt` files the two `neg_fib` cases need
were copied from `walnut-java`'s own `Custom Bases/` — they are Walnut's data files,
carried under the same GPLv3 attribution as every other fixture from that repo. Note
there is deliberately **no** `lsd_neg_fib*` file: real Walnut ships none either, so
`?lsd_neg_fib` resolves on both engines only through `NumberSystem.loadAutomatonOrNull`'s
opposite-direction-complement-plus-reverse fallback, which is exactly what
`neg_fib_lsd_resolves_through_the_complement_fallback` is there to check.

The command file and session directory were deleted from the `walnut-java` checkout
afterward, matching every recipe above.

---

# Ground-truth capture: `java_bugfix_wb024_wb025.rs` (no `fixtures/` files — all inline)

Captured 2026-08-20 for `tests/java_bugfix_wb024_wb025.rs`, verifying `wr_core::regex`'s
port of WB-024's and WB-025's real upstream fix (`walnut-java` commit `59eda64`, branch
`bugfix/wb-024-025`) — **not mainline**, per this project's now-standard practice for
these follow-up units (see the `java_bugfix_wb002.rs`/`java_bugfix_wb010.rs` entries in
this project's own git history for the same discipline). This is also where the coverage
that used to live in `fixtures/reg/r19.txt`/`r50.txt`/`r58.txt` moved to, once WB-024's
fix meant none of those three `(alphabets, regex)` pairs builds an automaton any more (see
the `reg` corpus entry above, and `tests/java_bugfix_wb024_wb025.rs`'s own module docs for
the full reasoning on why removal-plus-inline-replacement, not an updated fixture file,
was the right move here).

Built in an isolated worktree, per this project's standing shared-checkout-safety rule:

```bash
git -C ~/dev/walnut-java worktree add /tmp/walnut-java-wb024025-resume bugfix/wb-024-025
cd /tmp/walnut-java-wb024025-resume
# (jar already built: target/Walnut-all.jar)
cat > "Command Files/resume_capture.txt" <<'CMD'
reg r19new {0,1} "2*";
reg r50new {0,1,2,3} {0,1} "[9,9][0,0]";
reg r50rev {0,1,2,3} {0,1} "[0,0][9,9]";
reg r58new {0,1} "[10]";
CMD
/opt/homebrew/opt/openjdk@17/bin/java -cp target/Walnut-all.jar Main.Prover \
    resume_capture.txt >stdout.txt 2>stderr.txt
git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb024025-resume --force
```

`stderr.txt` was empty. `stdout.txt` (up to the REPL banner that follows the command
file):

```
reg r19new {0,1} "2*";
digit 2 in position 0 of a regular-expression vector is not in that input's alphabet: [0, 1]
reg r50new {0,1,2,3} {0,1} "[9,9][0,0]";
digit 9 in position 0 of a regular-expression vector is not in that input's alphabet: [0, 1, 2, 3]
reg r50rev {0,1,2,3} {0,1} "[0,0][9,9]";
digit 9 in position 0 of a regular-expression vector is not in that input's alphabet: [0, 1, 2, 3]
reg r58new {0,1} "[10]";
digit 10 in position 0 of a regular-expression vector is not in that input's alphabet: [0, 1]
```

None of `r19new.txt`/`r50new.txt`/`r50rev.txt`/`r58new.txt` were written to
`Automata Library/` — each command errors out before writing anything, so there is
nothing to copy into `fixtures/`. The test asserts this captured text verbatim through
`wr_cli::prover::Prover::read_buffer`, the same dispatch path the `reg` CLI command
actually uses (per `java_bugfix_wb010.rs`'s own established practice), not just through
`wr_core::regex::determine_encoded_regex` directly.

WB-025's boundary (`validateOffsetEncodableAlphabetSize`'s tightened `65408` limit — an
earlier revision of both the Java fix and this port used `65407`, one less than correct;
see `crates/wr-core/src/regex.rs`'s `validate_offset_encodable_alphabet_size` doc for the
full arithmetic and the fixup commit `446dab2`) was **not** re-captured through this
recipe — driving a 65409-track (or -symbol) alphabet declaration through `reg`'s own
textual grammar is impractically slow/parser-hostile at that scale. It is instead
independently verified by the fixed jar's own committed `Automata/FA/BricsConverterTest
.java` (added in `59eda64`, corrected for the same off-by-one in `446dab2`):
`./mvnw -q -Dtest=BricsConverterTest test` (run live, JDK 17, from an isolated worktree)
passed cleanly, pinning the exact `65408`/`65409` boundary and message text this port's
own `wb_025_*` tests in `crates/wr-core/src/regex/tests.rs` assert. Note
`Main/Commands/RegTest.java` does NOT cover WB-025 at all (four WB-024 tests only) — WB-025
has zero `reg`-command-level coverage on either engine; see
`tests/differential/tests/java_bugfix_wb024_wb025.rs`'s module docs for the same
correction (an earlier draft of this file wrongly claimed `RegTest` covered it too).

The command file and worktree were removed afterward, matching every recipe above.

---

# Ground-truth capture: `java_bugfix_wb032.rs` (plus a re-capture of `fixtures/convert_ns/b10msd1000.txt`)

Captured 2026-08-20 for `tests/java_bugfix_wb032.rs`, verifying `wr_core::logicalops`'s
port of WB-032's real upstream fix (`walnut-java` commit `18b7c4b`, branch
`bugfix/wb-032`) — **not mainline**, per this project's now-standard practice for these
follow-up units (see the `java_bugfix_wb002.rs`/`java_bugfix_wb010.rs` entries in this
project's own git history for the same discipline).

`bugfix/wb-032` was already checked out at the main `~/dev/walnut-java` working tree when
this was captured, so the worktree below is added by commit hash (detached), not by
branch name, to avoid git's "branch already checked out" refusal:

```bash
git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb032 18b7c4b
cd /tmp/walnut-java-wb032
./mvnw -q clean package -DskipTests -Pfat-jar

# -- Case 1: the regrouping direction (msd_10 -> msd_1000, was silently msd_100) --
cp <repo>/tests/differential/fixtures/convert_ns/base10.txt "Automata Library/wb032base10.txt"
cat > "Command Files/wb032_capture.txt" <<'EOF'
convert $wb032b10msd1000 msd_1000 $wb032base10;
EOF
java -jar target/Walnut-all.jar wb032_capture.txt < /dev/null

# -- Case 2: the ungrouping direction (msd_1000 -> msd_10, used to crash) --
cat > "Automata Library/wb032eps1000.txt" <<'EOF'
msd_1000

0 1
EOF
cat > "Command Files/wb032_capture2.txt" <<'EOF'
convert $wb032eps1000to10 msd_10 $wb032eps1000;
EOF
java -jar target/Walnut-all.jar wb032_capture2.txt < /dev/null

git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb032 --force
```

`java`/`mvnw` above actually ran under a JDK 17+ toolchain
(`/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1`) — the shell's default
`java` resolves to a JDK 11 too old for this project's class file version.

Case 1's output (`Session/<timestamp>/Automata Library/wb032b10msd1000.txt`, header
`msd_1000`, ~3,000 lines for the full `0..1000` alphabet) was copied over
`tests/differential/fixtures/convert_ns/b10msd1000.txt` as a straight overwrite —
that fixture's own re-capture, since it is the SAME operation (`convert … msd_1000 …`
against the same `base10.txt` source content) `tests/differential/tests/convert_ns.rs`'s
own capture recipe already used, now run against the fixed jar instead of mainline's, per
that file's own updated module docs. The command STRING differs cosmetically — this
worktree's actual capture, per the recipe above, was `convert $wb032b10msd1000 msd_1000
$wb032base10;`, using this file's own `wb032`-prefixed automaton names rather than
`convert_ns.rs`'s original `$b10msd1000`/`$base10` — not a byte-identical rerun of that
file's exact command text. `tests/differential/tests/java_bugfix_wb032.rs`'s own Case 1
test reuses that same fixture as its comparison target rather than duplicating a
~3,000-line automaton inline.

Case 2's output (`Session/<timestamp>/Automata Library/wb032eps1000to10.txt`, ~20 lines)
is inlined directly in `java_bugfix_wb032.rs` — see that file's own module docs for the
full text and the reasoning (state `0` rejects every digit into a non-accepting sink,
so the converted language stays `{ε}`, matching the untotalized `msd_1000` source).

The command files and hand-authored automaton files were deleted from the isolated
worktree afterward, matching every recipe above.

---

# Ground-truth capture: `java_bugfix_wb021.rs` (plus a re-capture of
`crates/wr-io/tests/fixtures/writer_true.ba`/`writer_false.ba`)

Captured 2026-08-20 for `tests/differential/tests/java_bugfix_wb021.rs`, verifying
`wr_io::writer`'s port of WB-021's real upstream fix (`walnut-java` commit `c0d7fff`,
branch `bugfix/wb-021`) — **not mainline**, per this project's now-standard practice for
these follow-up units.

`bugfix/wb-021` was already checked out at the main `~/dev/walnut-java` working tree
when this was captured (same situation `java_bugfix_wb032.rs`'s own recipe hit), so the
worktree below is added by commit hash (detached), not by branch name, to avoid git's
"branch already checked out" refusal:

```bash
git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb021 c0d7fff
cd /tmp/walnut-java-wb021
./mvnw -q clean package -DskipTests -Pfat-jar

cat > "Command Files/wb021_capture.txt" <<'EOF'
eval wb021true "?msd_2 Ex x = 1";
eval wb021false "?msd_2 Ex (x = 1 & x = 2)";
export $wb021true BA;
export $wb021false BA;
EOF
java -jar target/Walnut-all.jar wb021_capture.txt < /dev/null

git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb021 --force
```

`java`/`mvnw` above actually ran under a JDK 17+ toolchain
(`/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1`) — the shell's default
`java` resolves to a JDK 11 too old for this project's class file version.

Console output confirmed both `eval`s landed on the trivial automaton this test needs:
`eval wb021true "?msd_2 Ex x = 1";` prints `TRUE` (a satisfiable formula under one `∃`,
no free variables left); `eval wb021false "?msd_2 Ex (x = 1 & x = 2)";` prints `FALSE`
(an unsatisfiable conjunction under one `∃`).

`Session/<timestamp>/Result/wb021true.ba` (2 bytes, `"0\n"`) was copied over
`crates/wr-io/tests/fixtures/writer_true.ba` — that fixture's own re-capture against the
fixed jar. Byte-identical to what was there before (TRUE's export is unchanged by the
fix, confirming that half of the claim directly).

`Session/<timestamp>/Result/wb021false.ba` (**0 bytes**) was copied over
`crates/wr-io/tests/fixtures/writer_false.ba`, replacing what used to be the same
`"0\n"` as the TRUE fixture — this is the actual behavior change WB-021's fix makes,
confirmed live rather than assumed from reading the Java diff alone.

`tests/differential/tests/java_bugfix_wb021.rs`'s own test reruns the same two `eval`s
plus `export … BA;` through the real `wr-cli` dispatch path and asserts the written
`Result/*.ba` files are byte-identical to the two captures above (inlined as `&[u8]`
constants, not re-read from the `crates/wr-io` fixtures — the two crates' test fixtures
are independent copies of the same captured bytes, matching this project's established
practice of not creating a cross-crate test-fixture dependency for a two-byte/zero-byte
constant).

The command file was deleted from the isolated worktree afterward, matching every
recipe above.

---

# Ground-truth capture: `java_bugfix_wb038.rs`

Captured 2026-08-20 for `tests/differential/tests/java_bugfix_wb038.rs`, verifying
`wr_io::reader`'s port of WB-038's real upstream fix (`walnut-java` commit `601a9d2`,
branch `bugfix/wb-038`, stacked on `bugfix/wb-021`) — **not mainline**, per this
project's now-standard practice for these follow-up units.

`bugfix/wb-038` was already checked out at the main `~/dev/walnut-java` working tree when
this was captured (the same situation `java_bugfix_wb021.rs`/`java_bugfix_wb032.rs` hit),
so the worktree below is added by commit hash (detached), not by branch name:

```bash
git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb038 601a9d2
cd /tmp/walnut-java-wb038
./mvnw -q clean package -DskipTests -Pfat-jar

printf 'msd_2\n0 0\n0 -> 0\n1 -> 1\n1 1\n0 -> 0\n5 -> 1\n'  > "Automata Library/wb038fw.txt"
printf ' lsd_2\n0 1\n20 -> 0\n'                             > "Automata Library/wb038fy.txt"
printf 'msd_2 msd_2\n0 1\n5 1 -> 0\n'                       > "Automata Library/wb038alias.txt"
printf 'msd_2\n0 1\n* -> 0\n'                               > "Automata Library/wb038wild.txt"
printf '{0, 1}\n\n0\n0 -> 0 / 0\n1 -> 1 / 1\n\n1\n0 -> 1 / 1\n5 -> 0 / 0\n' \
                                                      > "Transducer Library/wb038td.txt"

cat > "Command Files/wb038_capture.txt" <<'EOF'
def wb038e "?msd_2 $wb038fw(x)";
eval wb038b "?lsd_2 $wb038fy(x)";
eval wb038al "?msd_2 $wb038alias(x,y)";
eval wb038w "?msd_2 $wb038wild(x)";
transduce wb038tr wb038td T;
eval wb038alive "?msd_2 Ex x = 1";
EOF
java -cp target/Walnut-all.jar Main.Prover wb038_capture.txt \
    >stdout.txt 2>stderr.txt </dev/null

git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb038 --force
```

`java`/`mvnw` above actually ran under a JDK 17+ toolchain
(`/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1/Contents/Home`) — the
shell's default `java` resolves to a JDK 11 too old for this project's class file version,
and `JAVA_HOME` must point at the `Contents/Home` subdirectory or `mvnw` refuses to start.
`</dev/null` matters too: without it the process runs the command file and then blocks in
the interactive REPL.

`stderr.txt` was empty. `stdout.txt`, up to the REPL banner that follows the command file:

```text
def wb038e "?msd_2 $wb038fw(x)";
digit 5 in position 1 is not in the alphabet [0, 1] of that input: line 7 of file Automata Library/wb038fw.txt
eval wb038b "?lsd_2 $wb038fy(x)";
digit 20 in position 1 is not in the alphabet [0, 1] of that input: line 3 of file Automata Library/wb038fy.txt
eval wb038al "?msd_2 $wb038alias(x,y)";
digit 5 in position 1 is not in the alphabet [0, 1] of that input: line 3 of file Automata Library/wb038alias.txt
eval wb038w "?msd_2 $wb038wild(x)";
transduce wb038tr wb038td T;
digit 5 in position 1 is not in the alphabet [0, 1] of that input: line 9 of file Transducer Library/wb038td.txt
eval wb038alive "?msd_2 Ex x = 1";
Converted from brics:2 states - 5ms
____
TRUE
```

The surviving `Session/<timestamp>/Automata Library/` held `wb038w.txt` and
`wb038alive.txt` and **nothing else** — the wildcard file's `eval` and the final liveness
`eval` are the only two commands that produced any automaton. (`Result/` additionally held
a `*_log.txt` per command, failed ones included, because Walnut opens the log before
dispatching; that is why the tests assert on `Automata Library` contents rather than on
`Result/`.)

## Three further shapes from the same session

Asserted at the `wr-io`/`wr-cli` layer rather than in the differential file, because they
need no CLI to be meaningful:

```text
# Automata Library/fpos2.txt = "msd_2 msd_3\n0 1\n1 7 -> 0\n"   (bad digit, NON-first track)
digit 7 in position 2 is not in the alphabet [0, 1, 2] of that input: line 3 of file Automata Library/fpos2.txt

# Automata Library/fset.txt = "{0, 1, 3}\n0 1\n2 -> 0\n"        (explicit-set alphabet)
digit 2 in position 1 is not in the alphabet [0, 1, 3] of that input: line 3 of file Automata Library/fset.txt

# Automata Library/fund.txt = " lsd_2\n0 1\n20-> 11\n"          (bad digit AND undeclared dest)
digit 20 in position 1 is not in the alphabet [0, 1] of that input: line 3 of file Automata Library/fund.txt
# ...i.e. the DIGIT check wins: it runs inside the parse loop, validateDeclaredStates after it.
# (A negative digit reports the same way: "digit -1 in position 1 ...".)
```

The first two are pinned by `the_out_of_alphabet_message_matches_real_walnut_text` in
`crates/wr-io/src/reader.rs`, the third by
`an_out_of_alphabet_digit_is_rejected_before_the_undeclared_state_check` in the same file.

## The 20-command corrupt-file matrix

`crates/wr-cli/src/prover.rs`'s `a_corrupt_library_file_costs_one_command_not_the_process`
was re-measured against the same jar in the same session — its exact command sequence, run
over an `Automata Library/bad.txt` of
`"msd_2\n0 0\n0 -> 0\n1 -> 1\n1 1\n0 -> 0\n5 -> 1\n"` and a `Word Automata
Library/badw.txt` of the same shape with output `2` on state 1. **All twenty commands
printed the same single line** (`digit 5 in position 1 … line 7 of file Automata
Library/bad.txt`, or `… Word Automata Library/badw.txt` for the two DFAO rows), the
following `reg alive …;` still ran, and the session's `Automata Library` afterwards held
only `ok.txt` and `alive.txt` — where against the PRE-fix jar it also held
`cc, dv, ev, fl, i, lq, rv, st` and `rvw`.

The command files and the six hand-authored library files were deleted from the isolated
worktree afterward, matching every recipe above.

---

# Ground-truth capture: `java_bugfix_wb035.rs`

Captured 2026-08-21 for `tests/differential/tests/java_bugfix_wb035.rs`, verifying
`wr_core::transducer`'s port of WB-035's real upstream fix (`walnut-java` commit
`7f54eff`, branch `bugfix/wb-035`, stacked on `bugfix/wb-038` (`601a9d2`)) — **not
mainline**, per this project's now-standard practice for these follow-up units.

Unlike the recipes above, **both** the fixed commit and its parent were built and run.
This unit's whole claim is a before/after difference in a silent wrong answer, so the
"before" was measured here rather than taken from the upstream commit message.
`bugfix/wb-035` was already checked out at the main `~/dev/walnut-java` working tree, so
both worktrees are added by commit hash (detached), not by branch name:

```bash
git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb035     7f54eff
git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb035-pre 601a9d2
# in each worktree:
./mvnw -q clean package -DskipTests -Pfat-jar

printf 'msd_2\n0 0\n0 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n' > "Word Automata Library/WB035P.txt"
printf 'msd_2\n0 1\n0 -> 1\n\n1 2\n0 -> 1\n1 -> 0\n' > "Word Automata Library/WB035P12.txt"
printf 'msd_2\n0 2\n0 -> 1\n\n1 3\n0 -> 1\n1 -> 0\n' > "Word Automata Library/WB035P23.txt"
printf 'msd_2\n0 0\n0 -> 1\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n' > "Word Automata Library/WB035T.txt"
printf '{0, 1}\n\n0\n0 -> 0 / -1\n1 -> 0 / 1\n'    > "Transducer Library/WB035NEG.txt"
printf '{0, 1}\n\n0\n0 -> 0 / 5\n1 -> 0 / 1\n'     > "Transducer Library/WB035POS.txt"
printf '{1, 2}\n\n0\n1 -> 0 / 7\n2 -> 0 / 8\n'     > "Transducer Library/WB035SHIFT.txt"
printf '{0, 1, 2}\n\n0\n0 -> 0 / 4\n1 -> 0 / 0\n2 -> 0 / 8\n' > "Transducer Library/WB035COL.txt"
printf '{1, 2, 3}\n\n0\n1 -> 0 / 7\n2 -> 0 / 8\n3 -> 0 / 9\n' > "Transducer Library/WB035S123.txt"

cat > "Command Files/wb035_capture.txt" <<'EOF'
transduce wb035neg WB035NEG WB035P;
transduce wb035pos WB035POS WB035P;
transduce wb035tot WB035NEG WB035T;
transduce wb035shift WB035SHIFT WB035P12;
transduce wb035col WB035COL WB035P12;
transduce wb035s123 WB035S123 WB035P23;
transduce wb035rs RUNSUM2 WB035P;
eval wb035alive "?msd_2 Ex x = 1";
EOF
java -cp target/Walnut-all.jar Main.Prover wb035_capture.txt \
    >stdout.txt 2>stderr.txt </dev/null
# transduce results land in Session/<timestamp>/Word Automata Library/

git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb035     --force
git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb035-pre --force
```

`java`/`mvnw` above actually ran under a JDK 17+ toolchain
(`/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1/Contents/Home` — the
shell's default `java` resolves to a JDK 11 too old for this project's class file version,
and `JAVA_HOME` must point at the `Contents/Home` subdirectory or `mvnw` refuses to start).
`</dev/null` matters: without it the process runs the command file and then blocks in the
interactive REPL. Note the first `-Pfat-jar`-less build produces only
`target/walnut-8.0-SNAPSHOT.jar`; the `Main.Prover` invocation above needs
`target/Walnut-all.jar`, which is the `fat-jar` profile's output.

## The captured before/after

Both jars printed the same eight echoed command lines plus `Converted from brics:2 states`
/ `TRUE` for the final liveness `eval`. The difference is in what they wrote:

| command | pre-fix (`601a9d2`) | post-fix (`7f54eff`) |
|---|---|---|
| `wb035neg`   | `0 -1 / 0->1` · `1 1 / 0->1` — **`1 -> 0` deleted** | `0 -1 / 0->1` · `1 1 / 0->1, 1->0` |
| `wb035col`   | `0 0 / 0->1` · `1 8 / 0->1` — **`1 -> 0` deleted** | `0 0 / 0->1` · `1 8 / 0->1, 1->0` |
| `wb035shift` | *no file written* | `0 7 / 0->1` · `1 8 / 0->1, 1->0` |
| `wb035s123`  | `0 1 / 0->1, 1->2` · `1 9 / 0->1` · `2 7 / 0->2, 1->2` — **three states, real and dead swapped** | `0 8 / 0->1` · `1 9 / 0->1, 1->0` |
| `wb035pos`   | `0 5 / 0->1` · `1 1 / 0->1, 1->0` | identical (control) |
| `wb035tot`   | `0 -1 / 0->1, 1->1` · `1 1 / 0->1, 1->0` | identical (total input) |
| `wb035rs`    | 8 states, `[0,1,1,0,1,1,0,0]` | identical (shipped `RUNSUM2`) |

The fixed jar's `stderr.txt` was **empty**. The pre-fix jar's carried exactly one entry,
for `wb035shift`:

```text
java.lang.NullPointerException: Cannot invoke "it.unimi.dsi.fastutil.ints.IntList.getInt(int)" because the return value of "Automata.FA.Transitions.getNfaStateDests(int, int)" is null
	at Automata.Transducer.createMap(Transducer.java:400)
```

The seven post-fix files are copied byte-for-byte into
`tests/differential/fixtures/wb035/`, and are what `java_bugfix_wb035.rs` compares against
(by `wr_core::equiv` semantic equivalence plus a per-word DFAO output comparison — this is
not a writer-fidelity unit, so no byte comparison is made).

## Corpus reachability, measured separately

WB-035's entry used to claim the shipped corpus never reaches the dead-state branch at all.
That was checked rather than trusted, in this port rather than in Java: a throwaway sweep
ran `RUNSUM2`/`RUNSUM3`/`RUNSUM4` against every file in `~/dev/walnut-java/Word Automata
Library` that passes `transduce`'s single-track and output-alphabet-compatibility guards —
**95 combinations, of which 32 add a distinguished dead state**, covering 12 distinct word
automata (`F`, `FASQ`, `FASQ1`, `FTM`, `KP`, `LUCAS`, `NA`, `R`, `RF`, `TR`, `V`, `X4`).
All 95 results were byte-identical between this port's pre-fix and post-fix code, since
every one of them is in the safe-coincidence case (`{0, 1, …}` alphabet, non-negative
outputs, `min(M.O) == 0`). The upstream commit reports the same finding from a smaller
sweep (8 of 24). The throwaway probe was deleted; `docs/WALNUT-BUGS.md` WB-035 carries the
corrected fact.

The command files, the nine hand-authored library files and both worktrees were removed
afterward, matching every recipe above.

---

# Ground-truth capture: `java_bugfix_wb013.rs` / `java_bugfix_wb033.rs` / `java_bugfix_wb034.rs`

Captured 2026-08-21 for PR-12 of `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md` (WB-013 + WB-033 +
WB-034), against the FIXED branch `bugfix/wb-013-033-034` (commit `c75e630`, stacked on
`bugfix/wb-043`) — **not mainline**, per this project's now-standard practice for these
follow-up units. All three bugs share one root cause (`Automaton.NS`/`getNS().get(i)` is a
literal `null` for a `{...}`-declared track, and three call sites used to dereference it
unguarded) and one fix (a shared `NumberSystem.requireNumberSystem(ns, subject)` helper),
so all three were captured in one session:

```bash
cd ~/dev/walnut-java   # bugfix/wb-013-033-034, already built: target/Walnut-all.jar

# WB-033: a one-track {0,1} automaton (no msd_k/lsd_k) for `convert`.
cat > "Automata Library/wb033nsless.txt" <<'EOF'
{0,1}

0 1
0 -> 0
1 -> 0
EOF

# WB-034: a two-track (Thue-Morse-shaped) {0,1} word automaton for `transduce`,
# transduced through the repo's own shipped `Transducer Library/RUNSUM2.txt`.
cat > "Word Automata Library/wb034nsless.txt" <<'EOF'
{0,1}

0 0
0 -> 0
1 -> 1

1 1
0 -> 1
1 -> 0
EOF

# WB-013: a two-track word automaton, "msd_2 {0,1}" -- track 0 is a real number
# system, track 1 (indexed by the repeated variable i below) is not.
cat > "Word Automata Library/wb013T.txt" <<'EOF'
msd_2 {0,1}

0 0
0 0 -> 0
0 1 -> 0
1 0 -> 0
1 1 -> 0
EOF

cat > "Command Files/wb033_capture.txt" <<'EOF'
convert $wb033out msd_4 $wb033nsless;
EOF
cat > "Command Files/wb034_capture.txt" <<'EOF'
transduce wb034out RUNSUM2 wb034nsless;
EOF
cat > "Command Files/wb013_capture.txt" <<'EOF'
eval wb013out "wb013T[i][i] = @1";
EOF

java -jar target/Walnut-all.jar wb033_capture.txt < /dev/null > /tmp/wb033_out.txt 2> /tmp/wb033_err.txt
java -jar target/Walnut-all.jar wb034_capture.txt < /dev/null > /tmp/wb034_out.txt 2> /tmp/wb034_err.txt
java -jar target/Walnut-all.jar wb013_capture.txt < /dev/null > /tmp/wb013_out.txt 2> /tmp/wb013_err.txt
```

`convert`'s `$` sigil on BOTH names means "not a DFAO" — i.e. read/write the *plain*
Automata Library, not Word Automata Library (`ProverHelper.determineInLibrary`); this
matches the WB-033 entry's own trigger example. All three runs' `stderr.txt` was **empty**
(the fixed exception is a genuine, handled `WalnutException`).

## Captured stdout (each command echoed first, per `Prover.readBuffer`)

WB-033 (`convert $wb033out msd_4 $wb033nsless;`):
```text
the automaton being converted has no attached number system (its alphabet was declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)
```

WB-034 (`transduce wb034out RUNSUM2 wb034nsless;`):
```text
the automaton being transduced has no attached number system (its alphabet was declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)
```

WB-013 (`eval wb013out "wb013T[i][i] = @1";`) — printed TWICE, per `EvalDef.compute`'s own
catch-log-then-rethrow shape (`Logging.printTruncatedStackTrace(e)` on the original
exception, then a second `WalnutException` wrapping `message + "\n\t: char at " +
t.getPositionInPredicate()`, caught again by `Prover.dispatch`'s own top-level catch):
```text
the track indexed by the repeated variable i in wb013T has no attached number system (its alphabet was declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)
the track indexed by the repeated variable i in wb013T has no attached number system (its alphabet was declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)
	: char at 0
```
(that last line's leading whitespace is one literal tab character, `\t`.)

None of the three commands writes a result file (each errors out before ever writing one),
so there is no `.txt` fixture to capture — only the printed lines, exactly like the
closed-formula cases this file's `wb037`/`wb044` entries above already established this
convention for.

The three hand-authored library files and command files were deleted from the checkout
afterward, matching every recipe above.

---

# Ground-truth capture: `custom_base_repeated_index.rs`

Captured 2026-08-21, against the same fixed branch as the WB-013 entry above
(`walnut-java` `bugfix/wb-013-033-034`, commit `c75e630`), while closing the custom-base
PORT gap that WB-013's own entry in `docs/WALNUT-BUGS.md` tracks (three rounds of patch,
then a rewrite — see that entry).

Unlike every earlier recipe, this one is **fully reproducible from this repo alone**: every
input file is checked in under `fixtures/custom_base_repeated_index/`, so nothing has to be
re-authored by hand. The capture was run in a throwaway COPY of the `walnut-java` working
tree, not in the checkout itself.

```bash
SCRATCH=$(mktemp -d)
cd ~/dev/walnut-java     # bugfix/wb-013-033-034, target/Walnut-all.jar already built
for d in "Automata Library" "Command Files" "Custom Bases" "Help Documentation" \
         "Macro Library" "Morphism Library" "Session" "Test Results" \
         "Transducer Library" "Word Automata Library"; do
  mkdir -p "$SCRATCH/$d"; cp -R "$d/." "$SCRATCH/$d/"
done
cp target/Walnut-all.jar "$SCRATCH/"

F=~/dev/walnut-rs/tests/differential/fixtures/custom_base_repeated_index
cp "$F"/msd_bar_addition.txt "$F"/msd_baz_addition.txt \
   "$F"/msd_neg_3_addition.txt "$F"/msd_neg_3_less_than.txt "$SCRATCH/Custom Bases/"
cp "$F"/BAR2.txt "$F"/BAZ2.txt "$F"/NEG3.txt "$SCRATCH/Word Automata Library/"

cat > "$SCRATCH/Command Files/r4_capture.txt" <<'EOF'
eval barout "BAR2[i][i] = @1";
eval neg3out "NEG3[i][i] = @1";
eval bazout "BAZ2[i][i] = @1";
EOF

cd "$SCRATCH" && java -jar Walnut-all.jar r4_capture.txt < /dev/null
# stdout echoes the three commands and nothing else; stderr is empty.
S="Session/<timestamp>/Automata Library"
cp "$S/barout.txt"  "$F/expected_barout.txt"
cp "$S/neg3out.txt" "$F/expected_neg3out.txt"
cp "$S/bazout.txt"  "$F/expected_bazout.txt"
```

The three bases are deliberately shaped to hit the three previously-broken cases:

* **`msd_bar`** — alphabet `{0, 1, 5}` (legal: `NumberSystem`'s constructor requires only
  that `0` and `1` be present), `msd_bar_addition.txt` **only**, no all-representations
  `msd_bar.txt`. A non-contiguous alphabet, so a base fabricated from its cardinality is
  observably different.
* **`msd_neg_3`** — a `Custom Bases/` pair SHADOWING the programmatic negative base of the
  same name, over `{0, 1, 2, 3}` where the programmatic one is `{0, 1, 2}`. **Both**
  `_addition.txt` and `_less_than.txt` are needed: with only the adder, real Walnut
  refuses the base outright —
  `Inputs of _less_than.txt must have the same alphabet as the alphabet of inputs of _addition.txt : base msd_neg_3`
  — because `setLessThanAutomaton` falls back to the programmatic negative comparator over
  the *other* alphabet. Confirmed live, both ways.
* **`msd_baz`** — alphabet `{0, 1, 2}` (contiguous), `msd_baz_addition.txt` only. The case a
  name-keyed discriminator refused even though Walnut computes it.

Captured results (all three 1-state, all three now reproduced exactly by the port):

```text
barout.txt   ->  header `msd_bar`,   transitions 0/1/5 -> 0
neg3out.txt  ->  header `msd_neg_3`, transitions 0/1/2/3 -> 0
bazout.txt   ->  header `msd_baz`,   transitions 0/1/2 -> 0
```

For the record, the same three commands run against this port at commit `c27838c` (round
3's fix, the state this change replaces) produced:

```text
barout   walnut-rs port limitation (real Walnut computes this successfully): ...          [stderr + stdout]
neg3out  in computing cross product of two automaton, variables with the same label must have the same alphabet   [stdout only, i.e. a HANDLED WalnutException -- the wrong-answer-as-legitimate-output shape]
bazout   walnut-rs port limitation (real Walnut computes this successfully): ...          [stderr + stdout]
```

and wrote no result files at all.

---

# Ground-truth capture: `java_bugfix_wb014.rs` — a "divergence closed" unit, not a "port bug fixed" one

Captured 2026-08-22 for `tests/differential/tests/java_bugfix_wb014.rs`, `docs/
WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-14. Unlike every other entry in this file, this
capture is NOT verifying a Rust-side fix — `wr_core::numsys::NumberSystem::
with_custom_base_files` never reproduced WB-014's `ConcurrentModificationException` in the
first place (its constructor takes already-parsed automata and does no I/O, so it cannot
re-enter a name→`NumberSystem` cache the way Java's constructor does; see `docs/
WALNUT-BUGS.md` WB-014's "Rust port" bullet, and `wr_io::reader::
read_automaton_txt_with_custom_bases`'s own module docs, both unchanged by this unit). What
changed is that **Java's bug is now fixed too** (`walnut-java` commit `6580f71`, branch
`bugfix/wb-014`, stacked on `bugfix/wb-011`) — so the divergence WB-014's entry originally
recorded (Rust succeeds, Java crashes) is now closed: both engines succeed and compute the
same automaton. This capture is what proves that, rather than trusting the upstream commit
message.

`bugfix/wb-014` was already checked out at the main `~/dev/walnut-java` working tree when
this was captured (the same situation `java_bugfix_wb021.rs`/`java_bugfix_wb032.rs`/
`java_bugfix_wb035.rs`/`java_bugfix_wb038.rs` hit), so the worktree below is added by
commit hash (detached), not by branch name, to avoid git's "branch already checked out"
refusal:

```bash
git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb014 6580f71
cd /tmp/walnut-java-wb014
./mvnw -q clean package -DskipTests -Pfat-jar

# WALNUT-BUGS.md's exact minimal WB-014 repro, unchanged:
printf 'msd_2 msd_2 msd_2\n\n0 1\n0 0 0 -> 0\n' > "Custom Bases/msd_wrtest_addition.txt"

cat > "Command Files/wb014_capture.txt" <<'EOF'
eval wrtest1 "?msd_wrtest x=x";
eval wrtest3 "?msd_wrtest Ex x=x";
EOF
java -cp target/Walnut-all.jar Main.Prover wb014_capture.txt \
    >stdout.txt 2>stderr.txt </dev/null

git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb014 --force
```

`java`/`mvnw` above actually ran under a JDK 17+ toolchain (`/Users/nkohen/Library/Java/
JavaVirtualMachines/openjdk-19.0.1/Contents/Home` — the shell's default `java` resolves to
a JDK 11 too old for this project's class file version). `</dev/null` matters: without it
the process runs the command file and then blocks in the interactive REPL.

`stderr.txt` was empty (no `ConcurrentModificationException`, nothing at all).
`stdout.txt`, up to the REPL banner that follows the command file:

```text
eval wrtest1 "?msd_wrtest x=x";
eval wrtest3 "?msd_wrtest Ex x=x";
____
TRUE
```

`Session/<timestamp>/Automata Library/wrtest1.txt`:

```text
msd_wrtest

0 1
0 -> 0
1 -> 0
```

— the 1-state, output-1, self-looping-on-every-digit accept-all shape `x=x` should always
collapse to. Small enough to inline directly in `java_bugfix_wb014.rs` as
`WRTEST1_CAPTURED`, matching the "inline a small captured fixture" convention
`java_bugfix_wb032.rs`'s Case 2 and `java_bugfix_wb035.rs`'s dead-letter cases already use
— no `fixtures/wb014/` directory was created. `wrtest3.txt` (the closed `Ex x=x` case) was
also written by both engines but is not captured as a fixture, per this project's
established convention for a trivial closed-formula result (the `fixtures/u11/`/
`fixtures/lsd/` entries above): the printed `TRUE` verdict is the meaningful observable,
checked directly against `Automaton::fa::is_true_automaton()`.

**Before writing any test, the current release build of `walnut-rs` (`cargo build -p
wr-cli --release`) was run live against the identical repro**, to independently
re-confirm the "architecturally immune" claim `docs/WALNUT-BUGS.md` WB-014's entry made,
rather than trusting it: a fresh `--home-dir` tree with the same `Custom Bases/
msd_wrtest_addition.txt`, `eval wrtest1 "?msd_wrtest x=x";` and `eval wrtest3 "?msd_wrtest
Ex x=x";` through the `walnut-rs` binary produced, respectively, the identical
`Automata Library/wrtest1.txt` content shown above and `____`/`TRUE` on stdout — confirming
the claim held before any test was written to pin it.

The command file, the hand-authored `Custom Bases/` file, and the worktree were removed
afterward, matching every recipe above.

---

# Ground-truth capture: `java_bugfix_wb036.rs`

Captured 2026-08-22 for `tests/differential/tests/java_bugfix_wb036.rs`, verifying
`wr_core::morphism`'s message text now matches WB-036's real upstream fix (`walnut-java`
commit `732bec0`, branch `bugfix/wb-036`, stacked on `bugfix/wb-026` (`051208a`)) — **not
mainline**, per this project's now-standard practice for these follow-up units.

**Safety note (this capture is a repeat offender's exact trigger shape):** a prior agent
working on this bug accidentally overwrote two real shipped files in
`Word Automata Library/` (`P.txt`/`P2.txt`, real fixture content) by using the bare names
`P`/`P2` as `promote` destinations while working directly in the shared `~/dev/walnut-java`
checkout. This capture instead (a) runs in a **detached worktree**, never the shared
checkout's own tracked tree, and (b) prefixes every morphism/promote name with
`wb036scratch_`, which cannot collide with any real Library file.

`bugfix/wb-036` was already checked out at the main `~/dev/walnut-java` working tree when
this was captured (the same situation `java_bugfix_wb021.rs`/`java_bugfix_wb032.rs`/
`java_bugfix_wb035.rs`/`java_bugfix_wb038.rs`/`java_bugfix_wb014.rs` hit), so the worktree
below is added by commit hash (detached), not by branch name:

```bash
git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb036 732bec0
cd /tmp/walnut-java-wb036
export JAVA_HOME=/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1/Contents/Home
export PATH="$JAVA_HOME/bin:$PATH"
./mvnw -q clean package -DskipTests -Pfat-jar

# WB-036's own repro shape, plus the two control cases (docs/WALNUT-BUGS.md), all with
# scratch-prefixed names per the safety note above:
cat > "Command Files/wb036_capture.txt" <<'EOF'
morphism wb036scratch_badmor "0->05 1->10";
promote wb036scratch_out1 wb036scratch_badmor;
morphism wb036scratch_h2 "0->00 1->00";
promote wb036scratch_out2 wb036scratch_h2;
morphism wb036scratch_dualmor "0->5 1->0";
promote wb036scratch_out3 wb036scratch_dualmor;
EOF
java -cp target/Walnut-all.jar Main.Prover wb036_capture.txt \
    >stdout.txt 2>stderr.txt </dev/null

rm -f "Command Files/wb036_capture.txt"
rm -rf Session
git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb036 --force
```

`java`/`mvnw` above actually ran under a JDK 17+ toolchain (the shell's default `java`
resolves to a JDK 11 too old for this project's class file version, and `JAVA_HOME` must
point at the `Contents/Home` subdirectory or `mvnw` refuses to start). `</dev/null` matters
too: without it the process runs the command file and then blocks in the interactive REPL.

`stderr.txt` was empty (confirming the domain-gap case now renders as a HANDLED
`WalnutException` — message-only, nothing on stderr — not the old unhandled-JDK-exception
shape). `stdout.txt`, up to the REPL banner that follows the command file:

```text
morphism wb036scratch_badmor "0->05 1->10";
Defined with domain [0, 1] and range {0, 1, 5}promote wb036scratch_out1 wb036scratch_badmor;
A morphism's domain must cover every value referenced in its own images: found the value 5 in some image, but the domain only has 2 letters.
morphism wb036scratch_h2 "0->00 1->00";
Defined with domain [0, 1] and range {0}promote wb036scratch_out2 wb036scratch_h2;
morphism wb036scratch_dualmor "0->5 1->0";
Defined with domain [0, 1] and range {0, 5}promote wb036scratch_out3 wb036scratch_dualmor;
Number system msd_1 is not defined.
```

Three confirmations from this one capture, matching `docs/WALNUT-BUGS.md` WB-036's own
"Verified against a live-built jar" bullets exactly:
- **The WB-036 shape** (`wb036scratch_badmor`, domain `{0,1}` but an image referencing `5`)
  now reports the clean message above instead of the old bare
  `java.lang.IndexOutOfBoundsException: Index 2 out of bounds for length 2`, and no
  `wb036scratch_out1.txt` is written anywhere under `Session/<timestamp>/` (confirmed —
  only `wb036scratch_out2.txt`, the mirror-shape control below, exists under
  `Word Automata Library/`).
- **The MIRROR-shape control** (`wb036scratch_h2`, domain `{0,1}`, images only ever
  reference `0`) is completely unaffected: `promote` succeeds silently (no printed line —
  Java's `promote` prints nothing on success absent a `::` suffix), and
  `Session/<timestamp>/Word Automata Library/wb036scratch_out2.txt` reads
  ```text
  msd_2

  0 0
  0 -> 0
  1 -> 0
  ```
  — the 1-state, self-looping-on-both-digits shape `Morphism::to_word_automaton`'s own
  `to_word_automaton_tolerates_a_domain_wider_than_the_image_range` test (`crates/wr-core/
  src/morphism.rs`) already pins.
- **The ordering control** (`wb036scratch_dualmor`, `0->5 1->0`, both `msd_1`-shaped AND
  WB-036-shaped) still reports `Number system msd_1 is not defined.`, confirming Java's
  fix preserves the pre-existing precedence (its own commit message calls this out
  explicitly).

**The same three commands, re-run against a release build of this port
(`cargo build -p wr-cli --release`) over a freshly created, otherwise-empty
`--home-dir` tree** (same `wb036scratch_`-prefixed command file, copied verbatim) produced
**byte-identical** `stdout`/`stderr` (modulo the session-timestamp line) and a
byte-identical `wb036scratch_out2.txt` — confirming the message-text fix and the
already-correct catch-point/mirror-shape/ordering behavior together, before any test was
written to pin it.
