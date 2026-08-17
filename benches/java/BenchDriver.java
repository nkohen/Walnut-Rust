/* SPDX-License-Identifier: GPL-3.0-or-later
 *
 * Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
 *
 * The JVM half of U32's performance comparison (Phase 4).
 *
 * Like `tests/differential-gen/java/DiffGenDriver.java` — which this is deliberately modelled
 * on, and which it deliberately does NOT modify — this file lives in *walnut-rs*, not in
 * `walnut-java`'s tracked source: it is a capture recipe, compiled fresh against
 * `walnut-java/target/Walnut-all.jar` on every run. See `benches/README.md` for the recipe and
 * the methodology.
 *
 * The one thing this driver does that `DiffGenDriver` cannot is **repeat the same command
 * inside one warm JVM and report per-iteration nanoseconds**, which is what makes the Java
 * numbers comparable to Criterion's: no JVM startup in the measurement, and the JIT given the
 * same warm-up the Rust side gets for free from being a `--release` binary.
 *
 * It also dispatches the fixture's FULL command script (`[strategy 6 BRZ]eval test637 "…"::`),
 * not a bare `eval "<formula>";` wrapper — the metacommand prefix and the `::` suffix are both
 * load-bearing for fixture 637, whose whole point is that `[strategy 6 BRZ]` (gated on the
 * `::` detail-printing flag) picks Brzozowski over subset construction.
 *
 * Protocol (all records NUL-terminated, UTF-8):
 *   request  := command "\0" warmup "\0" measure "\0"
 *   response := status "\0" nanos_csv "\0" kind "\0" payload "\0" details "\0"
 *   status   := "ok" | "fatal"
 *   kind     := "automaton" | "true" | "false" | "error" | "none"
 *
 * `nanos_csv` holds exactly `measure` comma-separated `System.nanoTime()` deltas, one per
 * measured iteration; `kind`/`payload`/`details` describe the LAST measured iteration, so the
 * harness can check both engines computed the same thing before believing either timing.
 */

import Automata.Automaton;
import Automata.Writer.AutomatonWriter;
import Main.Prover;
import Main.Session;
import Main.TestCase;

import java.io.BufferedInputStream;
import java.io.ByteArrayOutputStream;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.PrintStream;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.nio.charset.StandardCharsets;
import java.util.List;

public final class BenchDriver {
    private BenchDriver() {
    }

    /** One dispatch's outcome, as the wire protocol's `kind`/`payload`/`details` triple. */
    private static final class Outcome {
        String kind = "none";
        String payload = "";
        String details = "";
    }

    public static void main(String[] args) throws Exception {
        // The protocol stream is the process's REAL stdout, grabbed before `System.out` is
        // muted -- same reasoning as DiffGenDriver: Walnut prints "____\nTRUE" and (with `::`)
        // the whole detail trace on `System.out`, and letting any of it reach the pipe would
        // corrupt every subsequent record. Muting it also removes console I/O from the
        // measured region, which the Rust side likewise does not pay (it writes to a sink).
        OutputStream protocol = new FileOutputStream(FileDescriptor.out);
        System.setOut(new PrintStream(OutputStream.nullOutputStream(), true, "UTF-8"));

        if (args.length < 2) {
            throw new IllegalArgumentException("usage: BenchDriver <sessionDir> <homeDir>");
        }
        // Exactly the two directories the Rust side passes to `Session::new`, so both engines
        // read the same seeded `Global` library and write to their own copy of the session
        // tree (`Session.setPathsAndNames(sessionDir, homeDir, globalSession)`,
        // `Main/Session.java:62`).
        Session.setPathsAndNames(args[0], args[1], false);

        InputStream in = new BufferedInputStream(System.in);
        while (true) {
            byte[] commandBytes = readRecord(in);
            if (commandBytes == null) {
                break; // clean EOF between records: the harness is done with us.
            }
            byte[] warmupBytes = readRecord(in);
            byte[] measureBytes = readRecord(in);
            if (warmupBytes == null || measureBytes == null) {
                break; // truncated request; nothing sensible to answer.
            }
            String command = new String(commandBytes, StandardCharsets.UTF_8);
            int warmup = Integer.parseInt(new String(warmupBytes, StandardCharsets.UTF_8));
            int measure = Integer.parseInt(new String(measureBytes, StandardCharsets.UTF_8));

            String status = "ok";
            StringBuilder nanos = new StringBuilder();
            Outcome last = new Outcome();
            try {
                for (int i = 0; i < warmup; i++) {
                    dispatchOnce(command);
                }
                for (int i = 0; i < measure; i++) {
                    long t0 = System.nanoTime();
                    Outcome o = dispatchOnce(command);
                    long dt = System.nanoTime() - t0;
                    if (nanos.length() > 0) {
                        nanos.append(',');
                    }
                    nanos.append(dt);
                    last = o;
                }
            } catch (Throwable t) {
                // An `Error` (OOM / StackOverflow) leaves the JVM untrustworthy; report it
                // distinctly so the harness discards the whole measurement rather than
                // averaging a wounded run into the table. (Ordinary `Exception`s never reach
                // here -- `dispatchOnce` turns them into a `kind = "error"` outcome, because a
                // command that legitimately errors is still a timeable workload.)
                status = "fatal";
                last = new Outcome();
                last.kind = "error";
                last.payload = t.getClass().getName()
                        + (t.getMessage() == null ? "" : (": " + t.getMessage()));
            }

            writeRecord(protocol, status);
            writeRecord(protocol, nanos.toString());
            writeRecord(protocol, last.kind);
            writeRecord(protocol, last.payload);
            writeRecord(protocol, last.details);
            // Flush per response: a streaming pipe protocol, and Java's default buffering
            // would otherwise batch responses and deadlock the harness.
            protocol.flush();
        }
        protocol.flush();
    }

    /**
     * One dispatch, in exactly the shape `IntegrationTest.runSpecificTest` (`:925-927`) uses
     * per fixture:
     *
     * <pre>
     *   Prover.mainProver = new Prover();
     *   Prover.mainProver.dispatchForIntegrationTest(command, msg);
     * </pre>
     *
     * <b>Assigning the static `Prover.mainProver` is load-bearing, not decoration.</b>
     * `DeterminizationStrategies.determinize` (`Automata/FA/DeterminizationStrategies.java:99`)
     * reaches the current command's metacommands through `Prover.mainProver.metaCommands` — so
     * a driver that dispatches on a local `new Prover()` without publishing it leaves
     * `[strategy 6 BRZ]` looking up an unrelated (stale) `MetaCommands`, silently falls back to
     * subset construction, and turns fixture 637 from a 130 ms Brzozowski run into a
     * 24-second, 155,153-state one. That is a measurement of the harness, not of Walnut.
     * (`tests/differential-gen`'s driver never hit this because it never emits a metacommand.)
     *
     * Java's `Prover` is a cheap object; the state that actually persists across dispatches is
     * static (`NumberSystem.numberSystemHash`, `Prover.currentEvalName`), which is the
     * counterpart of the Rust side's session-lifetime `Prover` — see `benches/README.md`
     * §"Warm on both sides".
     */
    private static Outcome dispatchOnce(String command) throws Exception {
        Outcome o = new Outcome();
        try {
            Prover.mainProver = new Prover();
            TestCase tc = Prover.mainProver.dispatchForIntegrationTest(command, "");
            if (tc == null) {
                o.kind = "none";
                return o;
            }
            o.details = tc.getDetails();
            List<TestCase.AutomatonFilenamePair> pairs = tc.getAutomatonPairs();
            if (pairs.size() != 1) {
                o.kind = "error";
                o.payload = "<unexpected automaton-pair count " + pairs.size() + ">";
                return o;
            }
            Automaton m = pairs.get(0).automaton();
            if (m == null) {
                o.kind = "error";
                o.payload = "<null automaton in pair>";
            } else if (m.fa.isTRUE_FALSE_AUTOMATON()) {
                o.kind = m.fa.isTRUE_AUTOMATON() ? "true" : "false";
            } else {
                StringWriter sw = new StringWriter();
                PrintWriter pw = new PrintWriter(sw);
                AutomatonWriter.writeTxtFormatToStream(m, pw);
                pw.flush();
                o.kind = "automaton";
                o.payload = sw.toString();
            }
        } catch (Exception e) {
            // `IntegrationTest.runSpecificTest`'s own catch arm (`:963-965`).
            o.kind = "error";
            o.payload = e.getMessage() == null ? e.getClass().getName() : e.getMessage();
        }
        return o;
    }

    /** Reads one NUL-terminated record, or null on a clean EOF before any byte arrived. */
    private static byte[] readRecord(InputStream in) throws java.io.IOException {
        ByteArrayOutputStream buf = new ByteArrayOutputStream();
        int c;
        while ((c = in.read()) != -1) {
            if (c == 0) {
                return buf.toByteArray();
            }
            buf.write(c);
        }
        return null;
    }

    private static void writeRecord(OutputStream out, String s) throws java.io.IOException {
        out.write(s.getBytes(StandardCharsets.UTF_8));
        out.write(0);
    }
}
