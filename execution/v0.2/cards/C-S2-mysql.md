## Card C-S2 — lane C (MySQL): read-only watermark, C1 binlog, C5 and C5→C1 handoff

TODO: S2-C-01, S2-C-02. Lane label: `c`. Spec: REQ-ENG-03, AT-ENG-01. Playbook docs/product/02 §5.
Builds on your S0/S1 work on your lane branch; rebase on v0.2/product-loops first.

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
