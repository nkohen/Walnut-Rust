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
