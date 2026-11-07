# S5-C-04 / S5-D-03 T3 and T4 validation (2026-11-05)

Status: all required T3/T4 cases pass on MySQL 5.7.44 (amd64, emulated),
8.0.44, 8.4.7, 9.5.0 and PostgreSQL 13.23, 14.24, 15.19, 16.15, 17.11, 18.6
on the final binary. One PostgreSQL 16 run failed in a cascade after a network
reconnect and passed on rerun (see Downgrades). Raw evidence stays in
git-ignored `evidence/<run-id>/`; this file is the summary.

Binary: `oo-load` built with `--features mysql-binlog` at `b12e61d3e` (Rust sources unchanged since `f261d618f`)
(rebased onto `v0.2/product-loops`), SHA-256
`c05a820051301da83b00c2ecc2ebe40b714d82c195d4f3da5c727211de7c9443`.
Images resolve only through the v0.2 lock (`runner.resolve`).

```sh
tools/oo-test/oo-test t3t4 --engine mysql --versions 5.7,8.0,8.4,9 --run-id n-s5-mysql-03 --binary <oo-load>
tools/oo-test/oo-test t3t4 --engine postgres --versions 13,14,15,16,17,18 --run-id n-s5-pg-03 --binary <oo-load>
tools/oo-test/oo-test t3t4 --engine postgres --versions 16 --run-id n-s5-pg16-04 --binary <oo-load>
```

| Run ID | Exit | SHA-256 of root `SHA256SUMS` |
|---|---|---|
| `n-s5-mysql-03` | 0 | `6bc6c9e0bed3d4c60f17c916532a01af1997008152b13f76bf0004fc77aa2b86` |
| `n-s5-pg-03` | 1 (PostgreSQL 16 only) | `0b2ead4b830795e21cdc6ba55f6edcbe80a6245262acab76986499d1db847025` |
| `n-s5-pg16-04` | 0 | `8c4c34a781ed47d28f606c744bf8561999757c5ea2fa6576e3812be69eaa70b8` |

## Results (case x version)

PostgreSQL 16 column: `n-s5-pg16-04`; all other columns: `n-s5-mysql-03`,
`n-s5-pg-03`.

| Case | My 5.7 | My 8.0 | My 8.4 | My 9.5 | PG 13 | PG 14 | PG 15 | PG 16 | PG 17 | PG 18 |
|---|---|---|---|---|---|---|---|---|---|---|
| source_restart_mid_chunk | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| network_drop_mid_c1 | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| network_drop_mid_c5 | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| target_restart_mid_publish | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| sigkill_mid_cycle | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| retention_expiry | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| slot_invalidation | n/a | n/a | n/a | n/a | pass | pass | pass | pass | pass | pass |
| gtid_purge | pass | pass | pass | pass | n/a | n/a | n/a | n/a | n/a | n/a |
| add_column | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| drop_column | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| rename_column | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| widen_type | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| add_table | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| drop_table | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| pk_change | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |

`slot_invalidation` covers `wal_removed` (max_slot_wal_keep_size) on 13-18
and `idle_timeout` (idle_replication_slot_timeout) on 18 only; on 13-17 the
idle variant is recorded as not applicable. `gtid_purge` covers GTID purge
and a GTID/binlog reset that recreates the durable file name and offset.

## Observed handling

- Snapshot faults: the faulted run ends with a closed code (`load_target`,
  `connection_lost`, `statement_failed`, `database`) or SIGKILL; the same
  run ID resumes, without duplicate chunk receipts, to zero divergence.
- C1/C5 faults: resume to zero divergence with monotonic durable positions.
  Network drops (20 s) end by closed client timeouts or are survived
  in-process.
- History loss refuses with the target unchanged, then a new generation
  reaches zero divergence: MySQL `sync_rebaseline_required_retention`
  (expiry, purge) and `mysql_lineage_or_purge_new_generation_rebaseline`
  (reset); PostgreSQL `rebaseline_required_slot_missing` /
  `rebaseline_required_slot_invalidated`.
- Schema change: MySQL C1 refuses every DDL
  (`binlog_statement_new_generation_rebaseline`); PostgreSQL C1 refuses
  column/key changes (`sync_rebaseline_required_lineage_or_scope`) and drops
  (`sync_rebaseline_required_relation`); C5 refuses column/key changes
  (`c5_rebaseline_required_lineage`). Add table is applied online by C5 and
  PostgreSQL C1. Keyless intermediate shapes refuse by name (MySQL C1
  `binlog_requires_key`/`repair_plan`, PostgreSQL C1
  `capture_requires_full_columns_and_primary_key`, PostgreSQL C5
  `ctid_requires_exported_snapshot`). Every refused path was rebaselined and
  verified equal (columns and rows) after further DML.

## Product fixes found by this suite

1. MySQL step chunks planned at exactly the row budget from an optimizer
   estimate; a low estimate (e.g. after restart) overflowed every chunk and
   exhausted splits (`chunk_over_budget`). Now 3/4 of the budget.
2. C5 replanned every cycle; growth past MAX(id) or estimate drift became
   `c5_rebaseline_required_plan`. C5 now keeps the baseline's key ranges;
   PostgreSQL adopts them (`adopt_keyset_plan`) instead of `stale_chunk`.
3. Metadata-only DDL (rename, int->bigint) left digests equal: C5 published
   stale columns (silent divergence). C5 lineage now includes a table shape
   (MySQL `table_shape`), PostgreSQL lineage includes columns and key.
4. PostgreSQL capture treated a server-side restart_lsn advance during peek
   as a foreign consumer (`slot_changed_during_capture` on a fresh slot).
5. Hardening: client-side bounds on oo-load's PostgreSQL control
   connection (connect, lineage, slot feedback).

## Downgrades and incidents

- `n-s5-pg-03` PostgreSQL 16: after the `network_drop_mid_c5` reconnect,
  every later loader connection failed fast (`sync_source_connect`,
  `snapshot_connect`), so five cases failed in a cascade. Not reproduced in
  4 targeted repetitions (`n-dev-19`) or the full rerun `n-s5-pg16-04`. The
  server-side cause was not captured; the harness now records sessions and
  FATAL lines on any connect refusal. Earlier dev runs saw Docker Desktop
  lose a published port after a long disconnect (restored by a container
  restart, recorded in `operator_recoveries`). Treated as unattributed and
  intermittent, not as a pass of that run.
- Harness defects found and fixed during development (not product): MySQL
  5.7 `FLUSH HOSTS` is binlogged (now `FLUSH LOCAL HOSTS`), 5.7 new table
  defaulted to latin1 (C1 correctly refuses `binlog_charset_unsupported`),
  a 240 s wait killed C5 runs in their duty-cycle sleep after publishing.
- The lane rule "no container start below 20 GB free" is enforced by a disk
  gate; one development repro started at 19 GB before the gate existed.
- The 2026-11-04 Rosetta incident window (12:55-13:10 UTC) did not overlap
  any MySQL 5.7 run here; MySQL T8 sampler changes do not apply (no
  SourceMeter in this suite).

## Gaps

- Compact bigint/int/varchar fixture, consistency `B`; not full type/scale
  or T7. SSH jump and toxiproxy throttling are not covered.
- A metadata-only DDL between a snapshot and the first C5 cycle is not
  detected (the snapshot records no shape).
- Rebaseline is a new binding/source ID; there is no in-place C1 rebaseline
  command, and dropped tables' old views remain (no garbage collection).
- MySQL `snapshot_requires_innodb` is reported for a dropped table.
- Oracle C5 now also keeps durable ranges; Oracle was not run here.
