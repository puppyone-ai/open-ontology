# Production impact

Engine-specific side effects, sizing and consistency notes for the T10
production-impact measurement (S6-H-02, D24). The method, the per-phase
latency/throughput tables and the sign-off are in the
[production impact guide](../guide/production-impact.md). The numbers here are
measured peaks on a 4-CPU / 8 GiB fixture source with 2,000,000 rows, not
production sizing.

## Read consistency contract

Keyed PostgreSQL and MySQL tables default to `snapshot: chunk_consistent`.
On MySQL each transferred range and its canonical digest are read in one
bounded read-only repeatable-read transaction. On PostgreSQL a keyed range is
read in key-ordered parts of about 25 ms of source work each (1,024 to 65,536
rows, adapted to the measured rate): every part's rows and its digest are read
in one short read-only repeatable-read transaction, parts are paced by the
duty cycle, and the range's proof is the sum of its parts' digests (the
digest is a row count plus two sums, so the parts add up exactly). Oracle
keyset chunks are read `AS OF` one SCN each and C5 digests the range `AS OF`
that same SCN. All three engines therefore prove a C5 payload against the
snapshots it was read from rather than against a later digest. Ranges, and on
PostgreSQL the parts of a range, can represent different instants; the next
C5 cycle reconciles changes. This is not a globally consistent snapshot.

`snapshot: single` keeps one long snapshot (on PostgreSQL every part joins
the same exported snapshot). PostgreSQL physical (no-key) tables and key
shapes the catalog cannot prove keep a per-table exported snapshot and can
still hold back `xmin` for the length of that table's read.

An initial load keeps the digest each keyed range proved in its read
transaction (`_oo_load.c5_load_digests`, separate from C5 state so a C1
binding is not mistaken for a C5 publication). The first C5 cycle after the
load therefore transfers only ranges whose digest changed instead of the
whole table; the lite scheduler still fully re-reads on its first cycle and
every 24th.

Keyed C5 keeps the baseline's key ranges while the key shape is unchanged.
On PostgreSQL a C5 discovery digest also runs as key-ordered parts of about
10 ms each. A range whose row count outgrows the chunk budget (typically the
open-ended append tail) is split at C5 discovery into children that each fit
in half the budget; the children replace their parent in the durable plan and
are all read and staged before publication. More than 64 splits of one table
in one cycle is a `table_budget` stale for that table. LOB-bearing Oracle
tables digest through the bounded row reader, so a grown range there still
reports `table_budget` until it is rebaselined.

## Measured side effects (E473)

Runs `t10-s6h02-postgres-17-r2` (A), `t10-s6h02-postgres-17-r3` (B),
`t10-s6h02-mysql-84-r2` and `t10-s6h02-oracle-23-c1-r1`; latency results are
in the [guide](../guide/production-impact.md).

| Engine (run) | Phase | CPU peak % | Engine counters (peak) |
| --- | --- | --- | --- |
| PostgreSQL 17 A | baseline | 34.4 | autovacuum_count=12; backend_xmin_age=0; n_dead_tup=920; slot_retained_wal_bytes=0 |
| PostgreSQL 17 A | initial | 85.5 | autovacuum_count=13; backend_xmin_age=55; n_dead_tup=820; slot_retained_wal_bytes=0 |
| PostgreSQL 17 A | initial_duty100 | 84.8 | autovacuum_count=14; backend_xmin_age=20; n_dead_tup=704; slot_retained_wal_bytes=0 |
| PostgreSQL 17 A | c1 | 42.4 | autovacuum_count=24; backend_xmin_age=8; n_dead_tup=1293; slot_retained_wal_bytes=10040744 |
| PostgreSQL 17 A | c5_idle | 57.0 | autovacuum_count=30; backend_xmin_age=208; n_dead_tup=916; slot_retained_wal_bytes=0 |
| PostgreSQL 17 A | c5_changed | 57.6 | autovacuum_count=36; backend_xmin_age=13; n_dead_tup=926; slot_retained_wal_bytes=0 |
| PostgreSQL 17 A | c5_full_idle | 58.3 | autovacuum_count=42; backend_xmin_age=51; n_dead_tup=758; slot_retained_wal_bytes=0 |
| PostgreSQL 17 A | c5_full_changed | 53.5 | autovacuum_count=46; backend_xmin_age=20; n_dead_tup=720; slot_retained_wal_bytes=0 |
| PostgreSQL 17 B | baseline | 32.3 | autovacuum_count=12; backend_xmin_age=0; n_dead_tup=1083; slot_retained_wal_bytes=0 |
| PostgreSQL 17 B | c5_idle | 55.3 | autovacuum_count=18; backend_xmin_age=230; n_dead_tup=957; slot_retained_wal_bytes=0 |
| PostgreSQL 17 B | c5_changed | 56.9 | autovacuum_count=24; backend_xmin_age=24; n_dead_tup=847; slot_retained_wal_bytes=0 |
| PostgreSQL 17 B | c5_full_idle | 57.2 | autovacuum_count=31; backend_xmin_age=44; n_dead_tup=970; slot_retained_wal_bytes=0 |
| PostgreSQL 17 B | c5_full_changed | 67.8 | autovacuum_count=38; backend_xmin_age=57; n_dead_tup=917; slot_retained_wal_bytes=0 |
| MySQL 8.4 | baseline | 27.7 | Innodb_history_list_length=540; Threads_running=7; purge_backlog_transactions=540 |
| MySQL 8.4 | initial | 53.2 | Innodb_history_list_length=153; Threads_running=10; purge_backlog_transactions=153 |
| MySQL 8.4 | initial_duty100 | 73.4 | Innodb_history_list_length=139; Threads_running=7; purge_backlog_transactions=139 |
| MySQL 8.4 | c1 | 79.9 | Innodb_history_list_length=532; Threads_running=66; purge_backlog_transactions=532 |
| MySQL 8.4 | c5_idle | 80.5 | Innodb_history_list_length=541; Threads_running=66; purge_backlog_transactions=541 |
| MySQL 8.4 | c5_changed | 60.8 | Innodb_history_list_length=532; Threads_running=66; purge_backlog_transactions=532 |
| MySQL 8.4 | c5_full_idle | 80.0 | Innodb_history_list_length=540; Threads_running=66; purge_backlog_transactions=540 |
| MySQL 8.4 | c5_full_changed | 77.3 | Innodb_history_list_length=500; Threads_running=66; purge_backlog_transactions=500 |
| Oracle 23 C1 | baseline | 237.2 | logminer_session_cpu_centiseconds=0; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=62662 |
| Oracle 23 C1 | c1 | 191.5 | logminer_session_cpu_centiseconds=6869; ora_01555_count=0; tuned_undoretention_seconds=900; undo_blocks=98984 |

PostgreSQL C1's peak slot-retained WAL fell from 174 MB (E472) to 10 MB, and
the reader's peak `backend_xmin` age during C1 from 2,031 to 8 transactions:
each batch now decodes a bounded span and the cursor keeps up. MySQL's
`Threads_running` peaks of 58-66 after C1 began coincide with the fixture's
disk stalls recorded by the fsync canary (see the guide).

Block IO is Docker's cumulative, rounded counter. Oracle undo blocks and tuned
undo retention cover the `V$UNDOSTAT` horizon, not only the phase; LogMiner CPU
is sampled from live sessions and can miss sessions that already exited.
MySQL's history-list length is purge backlog, not seconds of lag.

## Operator checklist

The DBA, not the product reader, changes source settings.

| Engine | Observe during initial / C1 / C5 | Sizing and response |
| --- | --- | --- |
| PostgreSQL 17 | Reader `backend_xmin` age, hot-table dead tuples, autovacuum count, slot-retained WAL bytes, C1 lag | Keyed tables no longer hold one snapshot across a load or cycle; each part of a range holds one for tens of milliseconds. Physical/no-key tables still hold one per table. C1 decodes at most one batch (8,192 changes) per slot peek and applied the fixture's 1,900 changes/s with the slot at most 10 MB behind. Size a bounded `max_slot_wal_keep_size` (and the disk behind it) from measured WAL generation times the longest expected consumer outage plus catch-up, with headroom, and alert on retained WAL. Unlimited retention (`-1`) leaves `safe_wal_size` unknown and C1 refuses it as `wal_retention_headroom_unproven`; T10's fixture uses a bounded 16GB, which is not a sizing recommendation. Reads back off while four or more other sessions wait on locks or LWLocks. Never disable vacuum to hide impact. |
| MySQL 8.4 | `Innodb_history_list_length`, `Threads_running`, foreground latency | Size the redo log and buffer pool for the write rate before measuring: with the 8.4 defaults (100 MB redo capacity, 128 MB buffer pool) the T10 fixture stalled 3-6 s on its own; T10 now runs with 4 GiB redo capacity and buffer pool and `innodb_io_capacity` 2000/4000 at default durability. Size binlog retention for load, outage and catch-up windows. The C1 handoff repair keeps its 100,000-key / 32 MiB bound: a larger repair refuses atomically and needs a rebaseline. T10's C1 binds at an aligned position on a quiescent fixture; do not assume alignment on a live primary. |
| Oracle 23 | Undo blocks, tuned undo retention, ORA-01555 count, LogMiner session CPU, redo log switch rate | Size undo from peak generation times the longest consistent-read interval (each keyset chunk reads `AS OF` one SCN for the length of that chunk). Keep `sync: auto` (C5). LogMiner C1 (opt-in) mines each batch from a floor that advances with the durable cursor (an observation of the oldest active transaction, persisted per binding), so retained redo is needed from that floor, not from the baseline; logs below it may be purged. If you opt in, measure the change rate first and size online redo logs so they do not switch every few seconds (a switch during a batch is retried, up to three times). In T10 (about 2,000 row changes/s offered) C1 applied about 50 changes/s with LogMiner session CPU peaking at 68.7 s sampled; it no longer misses its batch deadline, but the cursor falls behind and the floor stops advancing once it trails every observation. No ORA-01555 occurred. |

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
