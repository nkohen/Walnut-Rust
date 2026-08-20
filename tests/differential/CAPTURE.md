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
60 real `reg` outputs by `tests/reg_brics_regex.rs`. Same one-time-capture discipline as
above — no live JVM shellout from the test.

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
