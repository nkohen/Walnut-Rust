# Hooks — to adapt from ct-research

This project inherits its operating discipline from ct-research, but its hooks are
NOT copied verbatim: ct-research's hooks are math-research-specific (claim-auditor
gate keyed to conjecture ledgers, reflect-on-failure keyed to sage/walnut commands).

Port deliberately, and follow ct-research's `.claude/hooks/AUTHORING.md` BEFORE
wiring any hook: self-test via the REAL invocation path across all branches
(happy / trigger / bypass / failure), and make any guard fail-diagnosable, never
fail-silent.

Candidates worth adapting here (in rough priority):
1. A commit gate that BLOCKS a commit touching correctness-critical crates
   (wr-core/wr-numsys/wr-logic) unless the adversarial-reviewer loop ran this
   session — the port's analogue of the claim-auditor gate.
2. The recursive-`rm` / backgrounded-job guard from ct-research's `guard.py`
   (safety; low false-positive).
3. A pre-push check that `cargo test --workspace` is green.

Until adapted, these disciplines are enforced by convention (CLAUDE.md), not
mechanically.
