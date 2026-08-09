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
