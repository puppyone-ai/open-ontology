# Production impact

Engine-specific side effects, sizing and consistency notes for the T10
production-impact measurement (S6-H-02, D24). The method, the per-phase
latency/throughput tables and the sign-off are in the
[production impact guide](../guide/production-impact.md). The numbers here are
measured peaks on a 4-CPU / 8 GiB fixture source with 2,000,000 rows, not
production sizing.

## Read consistency contract

Keyed PostgreSQL and MySQL tables default to `snapshot: chunk_consistent`: each
transferred range and its canonical digest are read in one bounded read-only
repeatable-read transaction, with the configured row, byte and statement-time
limits and one-second cleanup. Oracle keyset chunks are read `AS OF` one SCN
each; C5 now digests the range `AS OF` that same SCN, so all three engines
prove a C5 payload against the snapshot it was read from rather than against a
later digest. Ranges can represent different instants; the next C5 cycle
reconciles changes. This is not a globally consistent snapshot.

`snapshot: single` keeps one long snapshot. PostgreSQL physical (no-key) tables
and key shapes the catalog cannot prove keep a per-table exported snapshot and
can still hold back `xmin` for the length of that table's read.

Keyed C5 keeps the baseline's key ranges while the key shape is unchanged. A
range whose row count outgrows the chunk budget (typically the open-ended
append tail) is split at C5 discovery into children that each fit in half the
budget; the children replace their parent in the durable plan and are all read
and staged before publication. More than 64 splits of one table in one cycle
is a `table_budget` stale for that table. LOB-bearing Oracle tables digest
through the bounded row reader, so a grown range there still reports
`table_budget` until it is rebaselined.

## Measured side effects (final run)

| Engine (run) | Phase | CPU peak % | Engine counters (peak) |
| --- | --- | --- | --- |
| Oracle 23 A | baseline | 105.5 | logminer_session_cpu_centiseconds=0; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=69441 |
| Oracle 23 A | initial | 172.3 | logminer_session_cpu_centiseconds=0; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=70883 |
| Oracle 23 A | initial_duty100 | 159.7 | logminer_session_cpu_centiseconds=0; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=71360 |
| Oracle 23 A | c1 | 168.1 | logminer_session_cpu_centiseconds=220; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=113014 |
| Oracle 23 A | c5_idle | 139.2 | logminer_session_cpu_centiseconds=0; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=126065 |
| Oracle 23 A | c5_changed | 135.5 | logminer_session_cpu_centiseconds=0; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=152399 |
| Oracle 23 A | c5_full_idle | 114.7 | logminer_session_cpu_centiseconds=0; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=166168 |
| Oracle 23 A | c5_full_changed | 133.8 | logminer_session_cpu_centiseconds=0; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=191601 |
| Oracle 23 B | baseline | 85.0 | logminer_session_cpu_centiseconds=0; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=66965 |
| Oracle 23 B | c1 | 163.1 | logminer_session_cpu_centiseconds=3261; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=105953 |
| MySQL 8.4 auto | baseline | 237.5 | Innodb_history_list_length=496; Threads_running=66; purge_backlog_transactions=496 |
| MySQL 8.4 auto | initial | 64.9 | Innodb_history_list_length=405; Threads_running=3; purge_backlog_transactions=405 |
| MySQL 8.4 auto | initial_duty100 | 88.9 | Innodb_history_list_length=376; Threads_running=60; purge_backlog_transactions=376 |
| MySQL 8.4 auto | c1 | 326.1 | Innodb_history_list_length=448; Threads_running=66; purge_backlog_transactions=448 |
| MySQL 8.4 auto | c5_idle | 71.3 | Innodb_history_list_length=304; Threads_running=3; purge_backlog_transactions=304 |
| MySQL 8.4 auto | c5_changed | 67.2 | Innodb_history_list_length=409; Threads_running=7; purge_backlog_transactions=409 |
| MySQL 8.4 auto | c5_full_idle | 64.9 | Innodb_history_list_length=272; Threads_running=19; purge_backlog_transactions=272 |
| MySQL 8.4 auto | c5_full_changed | 73.3 | Innodb_history_list_length=296; Threads_running=11; purge_backlog_transactions=296 |
| MySQL 8.4 low | baseline | 122.1 | Innodb_history_list_length=371; Threads_running=66; purge_backlog_transactions=371 |
| MySQL 8.4 low | initial | 36.9 | Innodb_history_list_length=357; Threads_running=4; purge_backlog_transactions=357 |
| MySQL 8.4 low | initial_duty100 | 68.5 | Innodb_history_list_length=291; Threads_running=5; purge_backlog_transactions=291 |
| MySQL 8.4 low | c1 | 157.5 | Innodb_history_list_length=605; Threads_running=63; purge_backlog_transactions=605 |
| MySQL 8.4 low | c5_idle | 49.0 | Innodb_history_list_length=337; Threads_running=4; purge_backlog_transactions=337 |
| MySQL 8.4 low | c5_changed | 47.9 | Innodb_history_list_length=221; Threads_running=4; purge_backlog_transactions=221 |
| MySQL 8.4 low | c5_full_idle | 50.2 | Innodb_history_list_length=252; Threads_running=3; purge_backlog_transactions=252 |
| MySQL 8.4 low | c5_full_changed | 49.2 | Innodb_history_list_length=344; Threads_running=5; purge_backlog_transactions=344 |
| PostgreSQL 17 | baseline | 31.5 | autovacuum_count=12; backend_xmin_age=0; n_dead_tup=1107; slot_retained_wal_bytes=0 |
| PostgreSQL 17 | initial | 51.9 | autovacuum_count=13; backend_xmin_age=332; n_dead_tup=902; slot_retained_wal_bytes=0 |
| PostgreSQL 17 | initial_duty100 | 82.7 | autovacuum_count=13; backend_xmin_age=276; n_dead_tup=765; slot_retained_wal_bytes=0 |
| PostgreSQL 17 | c1 | 121.3 | autovacuum_count=25; backend_xmin_age=2031; n_dead_tup=1169; slot_retained_wal_bytes=174398376 |
| PostgreSQL 17 | c5_idle | 61.5 | autovacuum_count=26; backend_xmin_age=255; n_dead_tup=990; slot_retained_wal_bytes=0 |
| PostgreSQL 17 | c5_changed | 60.0 | autovacuum_count=28; backend_xmin_age=159; n_dead_tup=807; slot_retained_wal_bytes=0 |
| PostgreSQL 17 | c5_full_idle | 62.4 | autovacuum_count=29; backend_xmin_age=240; n_dead_tup=950; slot_retained_wal_bytes=0 |
| PostgreSQL 17 | c5_full_changed | 61.5 | autovacuum_count=32; backend_xmin_age=171; n_dead_tup=791; slot_retained_wal_bytes=0 |

Block IO is Docker's cumulative, rounded counter. Oracle undo blocks and tuned
undo retention cover the `V$UNDOSTAT` horizon, not only the phase; LogMiner CPU
is sampled from live sessions and can miss sessions that already exited.
MySQL's history-list length is purge backlog, not seconds of lag.

## Operator checklist

The DBA, not the product reader, changes source settings.

| Engine | Observe during initial / C1 / C5 | Sizing and response |
| --- | --- | --- |
| PostgreSQL 17 | Reader `backend_xmin` age, hot-table dead tuples, autovacuum count, slot-retained WAL bytes, C1 lag | Keyed tables no longer hold one snapshot across a load or cycle; physical/no-key tables still do, per table. Size a bounded `max_slot_wal_keep_size` (and the disk behind it) from measured WAL generation times the longest expected consumer outage plus catch-up, with headroom, and alert on retained WAL. Unlimited retention (`-1`) leaves `safe_wal_size` unknown and C1 refuses it as `wal_retention_headroom_unproven`; T10's fixture uses a bounded 16GB (peak retained 174 MB), which is not a sizing recommendation. Never disable vacuum to hide impact. |
| MySQL 8.4 | `Innodb_history_list_length`, `Threads_running`, foreground latency | Size binlog retention for load, outage and catch-up windows. The C1 handoff repair keeps its 100,000-key / 32 MiB bound: a larger repair refuses atomically and needs a rebaseline. T10's C1 binds at an aligned position on a quiescent fixture; do not assume alignment on a live primary. |
| Oracle 23 | Undo blocks, tuned undo retention, ORA-01555 count, LogMiner session CPU, redo log switch rate | Size undo from peak generation times the longest consistent-read interval (each keyset chunk reads `AS OF` one SCN for the length of that chunk). No ORA-01555 occurred in any final phase. Keep `sync: auto` (C5). LogMiner C1 (opt-in) re-mines redo from the binding's transaction floor on every batch; it fell behind about 2,200 row changes/s within ten minutes. If you opt in, measure the change rate first, size online redo logs so they do not switch every few seconds (a switch during a batch is retried, up to three times), and rebaseline periodically. |

## Standby / replica option

A separately provisioned replica can be evaluated for initial and C5 reads;
T10 does not certify any replica topology. Confirm the connector's supported
read mode, source identity, permissions, schema, replay lag and required read
consistency first, and keep lag visible in freshness reporting. If a C1 stream
and a snapshot use different endpoints, prove position alignment and retention
across both; a replica snapshot does not match the primary's change position
by default.

For PostgreSQL include standby recovery conflicts and replay lag in the trial.
For MySQL verify the endpoint exposes the required binlog/GTID history before
choosing C1. For Oracle have the DBA confirm standby read availability and
licensing. None of these paths authorizes product writes to the source or
automatic source reconfiguration.
