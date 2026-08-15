# RESUME-HERE — Phase 3b: 11 of 12 units complete; U27 (golden corpus) remains

**2026-08-15 checkpoint.** All 11 implementation units of Phase 3b (U17–U26) are merged to
`master`, each through the full implementer → two-split-context-adversarial-reviewer → fixer →
verify → merge loop. `cargo test --workspace` is green, `cargo fmt`/`clippy` clean, as of commit
`e3a9847`. `docs/WALNUT-BUGS.md` is at **37 entries** (WB-001–WB-037), no gaps or duplicates
(verify again before filing a new one — `grep -n "^## WB-" docs/WALNUT-BUGS.md | awk -F'WB-'
'{print $2}' | awk '{print $1}' | sort -n | uniq -c | awk '$1>1{print}'`).

**Only U27 (the Tier-1 golden-corpus harness) remains** to close out Phase 3b's actual DESIGN.md
exit criterion ("Tier 1 green; eval/def/reg, and now everything else, work"). It has NOT been
started — no go-ahead requested yet, and it's a large, distinctly-scoped unit (per the original
plan: consumes `walnut-java/phase0-artifacts/subset-filter.json`'s 591 subset-relevant fixtures,
extended with several exclusion rules — deferred-OTF strategies, a transitive-dependency rule for
DROP-scope `split`/`rsplit` consumers, the 7 CAS-matrix fixtures checking equivalence-only, the 15
`details*` fixtures needing exact pre-minimization state-count parity per an earlier user
decision — spawns the real `wr-cli` binary per command file). Read the corrected plan at
`~/.claude/plans/zany-sauteeing-pudding.md` (outside this repo, in the user's plans directory) for
the full delta against the original `.claude/plans/synthetic-prancing-aurora.md`, and re-verify its
assumptions against current code before starting, the same way this phase's own start re-verified
Phase 3b's original assumptions.

## What landed in U17–U26 (chronological, each with real adversarial-review findings)

- **U17** (Morphism.java → `wr-core::morphism`): reviewers found a false doc claim about `range`'s
  iteration order (fixed: documented as an accepted display-only divergence) and a genuinely
  missing `Fa::canonized`-flag prerequisite for the *deferred* `toWordAutomaton` (documented, not
  yet built at the time — U24 later built it for real).
- **U19** (Search/ProductBFS.java → `wr-core::search`): both reviewers independently found
  `shortest_accepted_word`/`shortest_witness_word_product` silently gave WRONG answers on
  nondeterministic input (fixed with a hard `SearchError::NotDeterministic` precondition). Core
  BFS/pruning algorithm confirmed sound by both reviewers via property-testing.
- **U22** (HelpMessages.java + text tree → `wr-cli::help_messages`): reviewers found real DATA
  CORRUPTION in 2 of 34 copied help files (mis-decoded as ISO-8859-1; fixed to match real Walnut's
  own U+FFFD replacement-char behavior, logged WB-030), wrong newline/CRLF handling, and an
  unported tokenizer (fixed; surfaced a genuine Walnut quirk, WB-031, that the `Morphisms And Word
  Automata` group's help is unreachable via real CLI syntax).
- **U21** (Prover dispatch core + MetaCommands + WalnutException → `wr-cli::prover`): both
  reviewers independently converged on `currentEvalName` not being threaded from Java's sticky
  static (fixed). Logged WB-028 (`earlyExistTermination` dead code, traced via git history) and
  WB-029 (`Strategy.fromString` alias-table bug).
- **U20** (Transducer.java → `wr-core::transducer`): reviewer found an unlogged genuine Walnut bug
  (`msd[0]==None` NPEs in real Java, silently permitted by the port; fixed + logged as WB-034) and
  both reviewers found real test-gaps via mutation testing. Algorithm confirmed sound by extensive
  differential/mutation testing. (Its own genuine-bug find, the `minOutput` dual-use defect, was
  renumbered WB-028→WB-035 mid-session to resolve a collision with U21 — see the sed-corruption
  lesson below.)
- **U18** (convertNS → `wr-core::logicalops`): reviewer found a **correctness-fatal** bug —
  `truncated_log_ratio` used Rust's correctly-rounded `f64::ln` where Java's `Math.log` is NOT
  correctly rounded, causing ~150-340 real `(root,exponent)` pairs to silently diverge from real
  Java (fixed: ported FDLIBM's `__ieee754_log`, verified bit-for-bit against a real JVM over
  200,000 values). Both reviewers independently found a new live call site for the already-known
  WB-001 quirk. WB-032/033 logged.
- **U25** (Test.java → `wr-cli::test_command`): reviewers found a reachable panic in the now-`pub`
  `find_accepted` API, a dropped resource-cap contract `search.rs` explicitly handed off to this
  unit (fixed: added `MAX_NEEDED` cap + made `find_accepted` take `&mut Automaton` matching Java's
  in-place mutation), and — proven by mutation testing — a length-cap removal that hangs forever
  (now a dedicated regression test).
- **U26** (transduce command → `wr-cli::transduce`): reviewer found the original 500-state resource
  guard was ineffective by ~2 orders of magnitude on the WRONG axis (real cost driver is BFS depth,
  not state count; fixed with a deterministic step/state/word-length budget inside
  `wr-core::transducer` itself, with measured caps) and a reachable panic on a well-formed partial
  transducer file (fixed, following the WB-034 precedent).
- **U23** (batch A: combine/concat/union/intersect/star/reverse/rightquo/leftquo/describe/minimize/
  fixleadzero/fixtrailzero/macro): both reviewers converged on `union`/`intersect`/`concat`
  silently accepting operands on different custom-base number systems (a real fail-open bug; fixed
  with a proper architectural change — a per-track NS *name* now threaded through `wr-io`'s reader
  → `Automaton` → `wr-core`, not just a narrower approximation). Also fixed: 3 dispatch arms
  (`combine`/`rightquo`/`leftquo`) could hit process-killing panics on ordinary mismatched-alphabet
  input (added `catch_walnut_panic`, a scoped `catch_unwind` boundary in `walnut_exception.rs`);
  `combine`'s output-value parser silently swallowing an integer overflow.
- **U24** (batch B: morphism/image/promote/join/convert/inf/export — the first real exercise of
  `Morphism`/`convertNS`/`Infinite`/the writer through the actual dispatch loop): reviewer found a
  **correctness-fatal** bug — the newly-built `Morphism::to_word_automaton` dropped Java's
  `NumberSystem` construction *and its validation*, so `promote` silently succeeded on inputs
  (e.g. any 1-uniform morphism) where real Java cleanly errors "Number system msd_1 is not
  defined." (fixed). Also fixed: `canonize()`'s early-return ordering bug (both reviewers
  independently found it), the `canonized` flag's invalidation semantics not matching Java's
  `Fa`-object-identity model (audited every reset site), a one-sided WB-036 guard, a new
  process-killing panic in `join` on mismatched alphabets, and a wrong blanket error-classification
  rule. Logged WB-036/037.

## Process lesson from this phase, worth repeating for whoever does U27

**A careless whole-file `sed` during WB-number renumbering nearly clobbered an already-merged
entry.** Mid-phase, U20's and U21's independently-authored bug entries both claimed "WB-028" for
unrelated bugs. Resolving the collision with `sed 's/WB-028/WB-035/g'` matched every occurrence in
the file — including U21's *already-merged, unrelated* WB-028 heading, which shares the literal
substring. Caught immediately by grepping for duplicate `## WB-NNN` headings before committing, not
by the edit itself. **Always use precise `Edit`-tool replacements with unique surrounding context
for this kind of renumbering, never a bare find-and-replace across the whole file** — WB-numbers
recur in the doc heading, in source-code comments, AND in test function names (`wbNNN_...`), and a
renumbering touches all three, but a *different* pre-existing bug's number is an unrelated but
textually-identical string. Grep for duplicates immediately after every resolved
`docs/WALNUT-BUGS.md` conflict, no exceptions — this project's own convention, reinforced here.

Also: rebase conflicts in `docs/WALNUT-BUGS.md` should always be resolved by **concatenating both
sides' entries**, never picking one over the other — every conflict encountered this phase was two
sets of new entries added independently, both valid, both worth keeping.
