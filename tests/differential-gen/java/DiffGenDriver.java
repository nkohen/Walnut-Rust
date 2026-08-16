/* SPDX-License-Identifier: GPL-3.0-or-later
 *
 * Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
 *
 * A throwaway JVM oracle driver for the Tier-3 differential generator (U29).
 *
 * This file deliberately lives in *walnut-rs*, NOT in `walnut-java`'s tracked source: it is a
 * capture recipe in the same spirit as `tests/differential/CAPTURE.md`'s `U8Probe.java`, not
 * production Walnut code. It is compiled fresh against `walnut-java/target/Walnut-all.jar`
 * every time the harness runs; see `tests/differential-gen/CAPTURE.md` for the exact recipe
 * and for the wire protocol this implements.
 *
 * Protocol (all records NUL-terminated, UTF-8):
 *   request  := query_id "\0" query_string "\0"
 *   response := query_id "\0" kind "\0" payload "\0"
 *   kind     := "automaton" | "true" | "false" | "error" | "fatal"
 *
 * `query_id` is echoed verbatim so the Rust side can assert the pipe never desynchronizes.
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

public final class DiffGenDriver {
    private DiffGenDriver() {
    }

    public static void main(String[] args) throws Exception {
        // The protocol stream is the process's REAL stdout, grabbed before `System.out` is
        // muted. Walnut prints "____\nTRUE" (EvalDef.computeHeadless) and assorted progress
        // chatter on `System.out`; letting any of that reach the pipe would corrupt every
        // subsequent record, so `System.out` is redirected into a bit bucket for the whole
        // process lifetime. `System.err` is left alone (it goes to the harness's log).
        OutputStream protocol = new FileOutputStream(FileDescriptor.out);
        System.setOut(new PrintStream(OutputStream.nullOutputStream(), true, "UTF-8"));

        if (args.length > 0) {
            // One scratch session tree for the whole process. Headless `eval "...";` writes
            // nothing (EvalDef.computeHeadless), but `Session`'s static paths back
            // `Logging`'s global log and the custom-base lookup, so point them somewhere
            // harmless rather than at the user's real working directory.
            String scratch = args[0].endsWith("/") ? args[0] : args[0] + "/";
            Session.setPathsAndNames(scratch + "Session/", scratch, false);
        }

        InputStream in = new BufferedInputStream(System.in);
        while (true) {
            byte[] idBytes = readRecord(in);
            if (idBytes == null) {
                break; // clean EOF between records: the harness is done with us.
            }
            byte[] queryBytes = readRecord(in);
            if (queryBytes == null) {
                break; // truncated request; nothing sensible to answer.
            }
            String id = new String(idBytes, StandardCharsets.UTF_8);
            String query = new String(queryBytes, StandardCharsets.UTF_8);

            String kind;
            String payload;
            try {
                // A fresh `Prover` per query, exactly as `IntegrationTest.runSpecificTest`
                // (`:926`) does per fixture. Java's `Session` is entirely static, so this is
                // "fresh dispatch state over one shared session", not a fresh session.
                TestCase tc = new Prover().dispatchForIntegrationTest("eval \"" + query + "\";", "");
                if (tc == null) {
                    kind = "error";
                    payload = "<dispatchForIntegrationTest returned null>";
                } else {
                    List<TestCase.AutomatonFilenamePair> pairs = tc.getAutomatonPairs();
                    if (pairs.size() != 1) {
                        kind = "error";
                        payload = "<unexpected automaton-pair count " + pairs.size() + ">";
                    } else {
                        Automaton m = pairs.get(0).automaton();
                        if (m == null) {
                            kind = "error";
                            payload = "<null automaton in pair>";
                        } else if (m.fa.isTRUE_FALSE_AUTOMATON()) {
                            kind = m.fa.isTRUE_AUTOMATON() ? "true" : "false";
                            payload = "";
                        } else {
                            StringWriter sw = new StringWriter();
                            PrintWriter pw = new PrintWriter(sw);
                            AutomatonWriter.writeTxtFormatToStream(m, pw);
                            pw.flush();
                            kind = "automaton";
                            payload = sw.toString();
                        }
                    }
                }
            } catch (Exception e) {
                // `IntegrationTest.runSpecificTest`'s own catch arm (`:963-965`) compares
                // `e.getMessage()` exactly; do the same here so the Rust comparator can too.
                kind = "error";
                payload = e.getMessage() == null ? e.getClass().getName() : e.getMessage();
            } catch (Throwable t) {
                // `Error` (OOM, StackOverflow): the JVM's state is no longer trustworthy.
                // Report it distinctly so the harness records `skip-too-big` and restarts us
                // instead of silently treating it as a semantic divergence.
                kind = "fatal";
                payload = t.getClass().getName()
                        + (t.getMessage() == null ? "" : (": " + t.getMessage()));
            }

            writeRecord(protocol, id);
            writeRecord(protocol, kind);
            writeRecord(protocol, payload);
            // Flush per response: this is a streaming pipe protocol, and Java's default
            // buffering would otherwise batch responses and deadlock the harness.
            protocol.flush();
        }
        protocol.flush();
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
        // EOF, with or without a partial (unterminated) record: either way there is nothing
        // well-formed left to answer, so the caller stops rather than guessing.
        return null;
    }

    private static void writeRecord(OutputStream out, String s) throws java.io.IOException {
        out.write(s.getBytes(StandardCharsets.UTF_8));
        out.write(0);
    }
}
