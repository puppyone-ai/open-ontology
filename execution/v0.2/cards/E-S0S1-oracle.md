## Card E-S0S1 — lane E (Oracle): engine adapter, chunking, source protection

TODO: S0-E-01, S1-E-01. Lane label: `e`. Spec: REQ-ENG-01/02, AT-ENG-01. Decisions D2 (19c blocked), D8.

Read: docs/product/02, docs/product/05, docs/database-support/04-database-profiles.md Oracle sections,
`crates/oo-connectors/src/engine/` (interface, do not modify), existing `crates/oo-connectors/src/oracle/`
(feature `oracle`, oracledb crate) and tools/oracle-preview/.

Deliver:
1. `SourceEngine` for Oracle. probe(): `VersionRule` table 11g (non-CDB, CONTINUOUS_MINE, no identity),
   12c–19c (CDB/PDB, c## common user at CDB$ROOT for LogMiner, 19c no CONTINUOUS_MINE), 21c/23ai (no
   non-CDB; 23ai extra system tablespaces excluded). Requirements: ARCHIVELOG, supplemental logging
   (minimal + PK/ALL per table), SELECT on tables and V$LOGMNR_* / V$ARCHIVED_LOG / V$DATABASE, EXECUTE
   DBMS_LOGMNR, FLASHBACK for AS OF SCN; DBA statements. LogMiner dictionary = online_catalog only (D8, R1);
   redo_log_catalog (needs DBMS_LOGMNR_D.BUILD) reported disabled (R1w).
2. plan_chunks(): keyset compared by Oracle under its own NLS sort (never NLSSORT forcing), composite
   rewrite, EXPLAIN PLAN check of key index use; no-key heap tables: ROWID ranges from ALL_EXTENTS /
   DBA_EXTENTS + DBMS_ROWID.ROWID_CREATE (never create DBMS_PARALLEL_EXECUTE tasks), valid only inside one
   AS OF SCN snapshot.
3. read_chunk(): AS OF SCN per chunk; ORA-01555 → shrink and retry (bounded) continuing only from flushed
   rows/keys; Arrow to ChunkSink; sha256; CURRENT_SCN watermarks; empty-string = NULL preserved and
   flagged. watermark(), digest_range() (count + order-independent digest with NULL flags; 11g has no
   STANDARD_HASH — use what the version has).
4. Source protection: one session per task, duty cycle, client call timeout, pause on active-session
   threshold (V$SESSION when readable), no locks/objects.
5. Tests: unit tests; Docker `gvenzl/oracle-free:23-slim` (arm64), `gvenzl/oracle-xe:21-slim` and
   `gvenzl/oracle-xe:11` (amd64, slow start). 19c is blocked — do not attempt Oracle sign-in. Fixture under
   testing/engines/oracle/ (NUMBER PK, VARCHAR2 PK case/accent/trailing-space edges, composite, unique-only,
   no-key heap, ≥2,000,000 rows, growing table, empty strings). T1 per version under a concurrent writer:
   zero diff with SCN correction. Record T0/T1/T8. Reuse tools/oracle-preview's Oracle client setup and
   document it exactly if client libraries are required.

Write scope: crates/oo-connectors/src/oracle/**, crates/oo-connectors/tests/oracle_*, tools/oracle-preview/**,
testing/engines/oracle/** (except seed/). Lock/manifest only if unavoidable (report). Prefix `connectors/oracle`.
Test commands: `cargo test -p oo-connectors --features oracle --lib` plus your real-engine entry.
