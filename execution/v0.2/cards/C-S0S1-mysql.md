## Card C-S0S1 — lane C (MySQL family): engine adapter, chunking, source protection

TODO: S0-C-01, S1-C-01, S1-C-02. Lane label: `c`. Spec: REQ-ENG-01/02, AT-ENG-01.

Read: docs/product/02-ingestion-playbook.md (§1–4, §7), docs/product/05-version-matrix-and-testing.md,
`crates/oo-connectors/src/engine/{mod.rs,version.rs}` (the interface you implement — do not modify),
existing `crates/oo-connectors/src/mysql/` (chunk.rs AdaptiveRows, version.rs, reader.rs, source.rs),
tools/mysql-preview/CHUNKING.md.

Deliver:
1. `SourceEngine` for MySQL (feature `mysql`). probe(): `VersionRule` table for 5.7 / 8.0 / 8.4 / 9.x —
   auth plugin (caching_sha2_password vs mysql_native_password; 9.x has no native_password),
   `SHOW BINARY LOG STATUS` (8.4+) vs `SHOW MASTER STATUS`, tagged GTID `UUID:TAG:N` parsing,
   `expire_logs_days` removed in 8.4 (use binlog_expire_logs_seconds), binlog off by default in 5.7,
   checks for log_bin / binlog_format=ROW / binlog_row_image=FULL / gtid_mode, privileges
   (SELECT, REPLICATION SLAVE/CLIENT). Requirements carry DBA-runnable GRANT/SET statements, no credentials.
2. plan_chunks(): key shape detection (int PK, varchar PK + its collation, composite PK, non-null unique
   index, no key). Distribution factor (MAX-MIN+1)/rows in [0.05,100] → step split; otherwise keyset
   boundary probing; >1000 estimated chunks → 1/1000 sampling. Estimates from EXPLAIN or MIN/MAX, not
   information_schema TABLE_ROWS. All bound comparisons in MySQL under the column collation, never
   `COLLATE ..._bin`; composite rewrite `a>? OR (a=? AND b>?)`; EXPLAIN-verify key index use. No key →
   SingleRead. Growth beyond budget → split into new `plan_revision`.
3. read_chunk(): AdaptiveRows, Arrow to ChunkSink, sha256 over canonical Arrow IPC, low/high watermark
   (GTID set, else file/pos). watermark(); digest_range() = COUNT + BIT_XOR of two 64-bit halves of
   per-row MD5 over CONCAT_WS with ISNULL flags, per key range.
4. Source protection (S1-C-02): one connection per task, ReadBudget duty cycle, pause while
   Threads_running exceeds a threshold, server `max_execution_time` (SELECT) plus client timeout,
   session net_read/net_write timeouts, never LOCK/FLUSH/CREATE.
5. Tests: unit tests for dispatch, SQL builders, composite rewrite, planner. Real engines in Docker:
   `mysql:5.7.44` (amd64), `mysql:8.0`, `mysql:8.4`, `mysql:9` (pin exact patch tags). Synthetic fixture
   under testing/engines/mysql/ (int PK, varchar PK utf8mb4_general_ci with case/accent/trailing-space
   edges, composite PK, unique-only, no-key, ≥2,000,000-row table, growing table) — lane H will later
   provide a shared generator; keep yours replaceable. T1 per version: under a concurrent writer
   (insert/update/delete incl. PK update) chunked read + watermark correction = zero diff vs source
   (row count + canonical row hash per chunk). Record T0/T1/T8 (peak connections, duty cycle) per version.

Write scope: crates/oo-connectors/src/mysql/**, crates/oo-connectors/tests/mysql_*, testing/engines/mysql/**
(except testing/engines/mysql/seed/** which is lane H). crates/oo-connectors/Cargo.toml + Cargo.lock only
if a dependency is unavoidable (report it). Commit scope prefix: `connectors/mysql`.
Test commands: `cargo test -p oo-connectors --features mysql --lib`, plus your real-engine test entry.
