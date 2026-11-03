## Card C-commit-S2 — lane C: commit S0/S1, then S2

Step 0 (S0-C-01, S1-C-01, S1-C-02): your previous run left uncommitted, signed-off work (MySQL adapter,
capability rules, GTID, keyset, protection, live tests; evidence c-s0s1-20261104-live08/09). Commit it now
in logical commits (rules/capability; GTID codec; keyset planner; protection; engine adapter; live test
runner and fixtures). Common rule 9 applies: the file_worker_parent failures were fixed on the integration
branch by your Darwin patch (65fe11714); parse_file_cli
parent_deadline_kills_the_worker_and_the_next_call_completes is pre-existing. Then rebase onto
v0.2/product-loops (your branch has no commits yet beyond the base, so commit first, then
`git rebase v0.2/product-loops`). The shared fixture generator, gold questions and oo-test runner are now on
the integration branch (testing/engines/seed-common, testing/engines/gold, tools/oo-test); prefer them.
Then do the S2 card below.


TODO: S2-C-01, S2-C-02. Lane label: `c`. Spec: REQ-ENG-03, AT-ENG-01. Playbook docs/product/02 §5.
Requires: your S0/S1 work integrated (rebase on v0.2/product-loops first).

Deliver:
1. S2-C-01: read-only chunk watermarks (GTID set; file/pos when GTID off; replica_preserve_commit_order
   check when reading a replica) with chunk-window log correction (log wins over snapshot rows for keys
   seen in the window, tombstones prevent resurrection). Record L0 before the first full read and keep
   consuming the binlog into staging during the full read. Implement `ChangeSource` for MySQL
   (binlog ROW events via the existing tools/mysql-preview / job-worker C1 code where it exists — reuse,
   don't duplicate): earliest_available() from the actual oldest binlog on the server, not from settings.
2. S2-C-02: C5 range digests through `SourceEngine::digest_range` with the four layers (time hint, recent
   window, chunk digest, rare full read); a no-change cycle = zero payload, zero target rewrite, no new
   generation. C5→C1 handoff barrier for multiple sources/bindings (subscribe L0 → repair baseline bounded
   by L0 → log-first with tombstones → switch at L_end barrier); GTID lineage break or purged logs → an
   explicit new-generation rebaseline. Retention preflight: oldest available log vs longest cycle interval +
   recovery time; fail with a Requirement when short.
3. Tests per version 5.7/8.0/8.4/9.x: T1 with log correction; T2 C1 and C5 three cycles each incl. a
   no-change cycle (zero diff, monotonic positions); handoff test; retention-short test → explicit rebaseline.
Use lane H's shared fixture/oo-test/comparison oracle if integrated (testing/engines/seed-common,
tools/oo-test), else your own fixture.
Write scope: crates/oo-connectors/src/mysql/**, crates/oo-connectors/tests/mysql_*, testing/engines/mysql/**
(not seed/), and for C1 reuse tools/mysql-preview/** and tools/job-worker/src/native_mysql_*.rs. Prefix
`connectors/mysql` / `job-worker`.
