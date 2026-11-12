# Production impact

## Pending mitigation: no new production recommendation

The T10 harness now uses half the settled rate for automatic-rate measurements.
No new live T1/T2 or impact measurements accompany that change; retain the
existing operational cautions and do not interpret historical live-4 numbers
as results of the new method. The [partial mitigation handoff](../../tools/oo-test/T10-MITIGATE.md)
records the unresolved snapshot/digest proof and the MySQL 100,000-key handoff
limit, which cannot cover T10's two-million-row bulk table merely by raising
a harness setting. Snapshot defaults, source settings and safety caps remain
unchanged. S6-H-02 is still incomplete.

## Current measurement: T10-live-4

No engine passed every phase. See the [measured phase table](../guide/production-impact.md)
and [complete evidence report](../../tools/oo-test/T10-LIVE4.md). These are measured
peaks, not production sizing recommendations. Missing preparation samples are
unmeasured, never zero. Oracle A/B denote independent baseline comparisons.

| Engine | Phase | CPU peak % | Block read / write peak bytes | Engine-side peak counters |
| --- | --- | --- | --- | --- |
| Oracle A | baseline | 266.040 | 26200000.000 / 21800000000.000 | logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=900.000; undo_blocks=106813.000 |
| Oracle A | initial | 344.880 | 158000000.000 / 61400000000.000 | logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=985.000; undo_blocks=335919.000 |
| Oracle A | initial_duty100 | 347.830 | 160000000.000 / 75200000000.000 | logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=900.000; undo_blocks=420990.000 |
| Oracle B | baseline | 148.090 | 23200000.000 / 22400000000.000 | logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=900.000; undo_blocks=108118.000 |
| Oracle B | c5_idle | 265.150 | 51400000.000 / 27900000000.000 | logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=900.000; undo_blocks=136571.000 |
| Oracle B | c5_changed | unmeasured | unmeasured / unmeasured | unmeasured |
| Oracle B | c5_full_idle | 170.170 | 130000000.000 / 33600000000.000 | logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=1014.000; undo_blocks=166720.000 |
| Oracle B | c5_full_changed | unmeasured | unmeasured / unmeasured | unmeasured |
| Oracle B | c1 | unmeasured | unmeasured / unmeasured | unmeasured |
| MySQL 8.4.7 | baseline | 365.160 | 31800000.000 / 15100000000.000 | Innodb_history_list_length=465.000; Threads_running=66.000; purge_backlog_transactions=465.000 |
| MySQL 8.4.7 | initial | 78.480 | 31800000.000 / 15100000000.000 | Innodb_history_list_length=354.000; Threads_running=4.000; purge_backlog_transactions=354.000 |
| MySQL 8.4.7 | initial_duty100 | 77.860 | 31800000.000 / 15500000000.000 | Innodb_history_list_length=304.000; Threads_running=7.000; purge_backlog_transactions=304.000 |
| MySQL 8.4.7 | c1 | unmeasured | unmeasured / unmeasured | unmeasured |
| MySQL 8.4.7 | c5_idle | 388.740 | 31900000.000 / 35600000000.000 | Innodb_history_list_length=458.000; Threads_running=66.000; purge_backlog_transactions=458.000 |
| MySQL 8.4.7 | c5_changed | 376.320 | 154000000.000 / 40600000000.000 | Innodb_history_list_length=420.000; Threads_running=66.000; purge_backlog_transactions=420.000 |
| MySQL 8.4.7 | c5_full_idle | 377.340 | 194000000.000 / 57900000000.000 | Innodb_history_list_length=532.000; Threads_running=66.000; purge_backlog_transactions=532.000 |
| MySQL 8.4.7 | c5_full_changed | 377.830 | 434000000.000 / 69400000000.000 | Innodb_history_list_length=623.000; Threads_running=66.000; purge_backlog_transactions=623.000 |
| PostgreSQL 17.11 | baseline | 101.750 | 0.000 / 19600000000.000 | autovacuum_count=12.000; backend_xmin_age=0.000; n_dead_tup=1705.000; slot_retained_wal_bytes=0.000 |
| PostgreSQL 17.11 | initial | 107.730 | 0.000 / 20400000000.000 | autovacuum_count=13.000; backend_xmin_age=102158.000; n_dead_tup=76346.000; slot_retained_wal_bytes=0.000 |
| PostgreSQL 17.11 | initial_duty100 | 63.030 | 0.000 / 20400000000.000 | autovacuum_count=13.000; backend_xmin_age=0.000; n_dead_tup=2929.000; slot_retained_wal_bytes=0.000 |
| PostgreSQL 17.11 | c1 | 142.200 | 0.000 / 36000000000.000 | autovacuum_count=24.000; backend_xmin_age=8250.000; n_dead_tup=5028.000; slot_retained_wal_bytes=305079376.000 |
| PostgreSQL 17.11 | c5_idle | 294.660 | 0.000 / 40600000000.000 | autovacuum_count=28.000; backend_xmin_age=628178.000; n_dead_tup=472036.000; slot_retained_wal_bytes=0.000 |
| PostgreSQL 17.11 | c5_changed | 413.020 | 0.000 / 48200000000.000 | autovacuum_count=35.000; backend_xmin_age=1119964.000; n_dead_tup=840325.000; slot_retained_wal_bytes=0.000 |
| PostgreSQL 17.11 | c5_full_idle | 292.580 | 0.000 / 53000000000.000 | autovacuum_count=39.000; backend_xmin_age=628203.000; n_dead_tup=472160.000; slot_retained_wal_bytes=0.000 |
| PostgreSQL 17.11 | c5_full_changed | 410.740 | 0.000 / 63100000000.000 | autovacuum_count=58.000; backend_xmin_age=2121968.000; n_dead_tup=1591492.000; slot_retained_wal_bytes=0.000 |

Block IO is Docker's rounded cumulative counter, not phase IO. Undo counters
cover retained V$UNDOSTAT history; autovacuum counts include fixture activity.
History-list length is a purge backlog, not time lag. Live-session LogMiner CPU
samples cannot prove total CPU for exited sessions; C1 did not complete here.

PostgreSQL changed-C5 held reader xmin up to 1119964 / 2121968 transactions
and hot-table dead tuples up to 840325 / 1591492 (period 24 / 1). Both policies
exceeded D24. Its C1 slot peaked at 305079376 retained WAL bytes and invalidated
under the fixture's 256 MiB cap. Size retention for measured load, outage and
catch-up; do not treat the fixture cap as production sizing. Short-transaction
replacement must preserve C5 correction before publication and pass full T1/T2.

MySQL's history list peaked at 623 and Threads_running at 66 in the reduced-rate
run. The initial loads passed the relative budget, but C1 repair staging and
changed-C5 partial-sync errors remain blockers. Resolve those errors before
using the path; do not increase budgets blindly or equate partial sync with fresh.

Oracle A's interrupted load reached 335919 undo blocks in the retained history;
Oracle B idle full C5 reached 166720, with tuned retention 1014 seconds. Sampled
ORA-01555 was zero, but changed-C5/C1 preparation timed out and initial loads
were incomplete. There is no validated undo capacity or C1-versus-C5 CPU ratio.
Use measured undo generation times the longest consistent-read interval, plus
DBA-approved headroom, and confirm under peak writes. No init parameter changed.

Keep periodic full verification and low-write windows. Existing active-session
or Threads_running gates did not establish all-phase acceptance. Adaptive tuning
and short transactions remain unvalidated; no full-scale live T1/T2 ran in this
attempt. Replica/standby evaluation requires the identity, lag, position-alignment
and licensing checks in the operator checklist below. All owned resources are gone.

## Historical T10-live-3 status

No live D24 budget is verified for Oracle 23, MySQL 8.4, or PostgreSQL 17.
The T10-live-3 sustained-rate attempt failed calibration for Oracle and the
ten-minute baseline for MySQL/PostgreSQL, even after their extra 30% step.
No initial/C1/C5 phase or mitigation comparison is available. See the
[measured results](../guide/production-impact.md) and the
[engine x phase report](../../tools/oo-test/T10-LIVE3.md).

## T10-live-3 baseline side effects

| Baseline | CPU peak | Cumulative block IO read / write peak | Source counter peaks |
| --- | --- | --- | --- |
| mysql, settled | 233.10% | 31.8 MB / 17.300 GB | History list / purge backlog 513; Threads_running 66 |
| mysql, lower30 | 92.15% | 31.7 MB / 16.200 GB | History list / purge backlog 426; Threads_running 66 |
| postgres, settled | 72.33% | 0.0 MB / 18.900 GB | Reader xmin age 0; dead tuples 1074; autovacuum count 13; slot WAL 0 B |
| postgres, lower30 | 62.73% | 0.0 MB / 10.500 GB | Reader xmin age 0; dead tuples 1087; autovacuum count 10; slot WAL 0 B |

These are foreground-only baselines, not connector impact. All foreground
errors were queue rejections; metrics collection itself had no errors.
Oracle phase side effects were not measured because settling failed.
A fixture-only diagnosis found `DPY-6005` / `DPY-6000` / `ORA-12516` connection
failures. Sampled process high-water was 142 of 200 and session high-water
227 of 322; these counters do not establish the cause. Investigate listener
admission before raising limits or rerunning a production-impact benchmark.
The harness now closes trial connections; rate selection remains unchanged.

No valid default phase exceeded D24 because no valid comparison exists.
Retain the operator checklist below; no numeric retention sizing or changed
product default is justified by failed foreground-only baselines.

## Previous T10-live-2 baseline side effects

Runs `t10-live2-{oracle,mysql,postgres}-corrected-20261109` used native arm64
Oracle 23.5.0.24.07, MySQL 8.4.7 and PostgreSQL 17.11 in that order.
Each source was limited to 4 CPUs / 8 GiB and observed for 600 seconds.

| Engine | Peak counters | Peak cumulative container block IO, read / write |
| --- | --- | --- |
| Oracle | Undo 100,769 blocks; tuned retention 900 s; ORA-01555 0; live LogMiner CPU 0 centiseconds | 23.4 MB / 21.3 GB |
| MySQL | History list / purge backlog 472 transactions; Threads_running 34 | 31.7 MB / 14.9 GB |
| PostgreSQL | Reader xmin age 0; hot-table dead tuples 1,310; autovacuum count 11; retained slot WAL 0 bytes | 0 MB / 20.3 GB |

IO values are Docker's rounded cumulative container counters, not per-phase
IO deltas. Undo and autovacuum counters also include earlier fixture activity.
The baseline has no product reader or C1 miner, so zero reader/slot/miner
counters cannot establish their behavior during product phases. All foreground
errors were `scheduler_queue_full`; none is evidence of a connector-induced
database error. These failed baselines cannot size production retention.

## Operator checklist

These are measurement and mitigation instructions, not measured capacity or
retention guarantees. The DBA, not the product reader, changes source settings.

| Engine | Observe during initial / C1 / C5 | Sizing and response |
| --- | --- | --- |
| PostgreSQL 17 | Reader `backend_xmin` age, hot-table dead tuples, autovacuum count, slot-retained WAL bytes | Measure the longest read and WAL generation over the longest expected consumer outage plus catch-up. Reserve disk headroom for that interval and alert on retained WAL growth. If long snapshots hold back cleanup, evaluate short chunk transactions plus C5 correction before publication; do not disable vacuum to hide impact. |
| MySQL 8.4 | `Innodb_history_list_length`, `Threads_running`, foreground latency | History-list length measures purge backlog, not lag in seconds. Measure growth during the longest read and recovery after it. Investigate sustained growth before increasing concurrency. Size binlog retention for measured load, outage and catch-up windows, with DBA-approved headroom; retain the snapshot path until the short-transaction alternative passes T1/T2. |
| Oracle 23 | Undo blocks, tuned undo retention, ORA-01555 counts, live LogMiner-session CPU | Measure peak undo generation and the longest required consistent-read interval. Use their product plus workload-specific headroom as a starting capacity estimate, then validate under peak writes; no fixed GB or seconds value is established here. If explicit LogMiner C1 exceeds budget, evaluate the default C5 path and remeasure its digest-scan cost. |

Oracle's sampled `V$UNDOSTAT` totals cover the retained history, not only the
phase. Compare before/after as well as peaks. Sampled live-session LogMiner CPU
can miss sessions that already exited. Never use these counters alone to claim
causal attribution or a complete CPU total.

## Standby / replica option

A separately provisioned replica can be evaluated for initial and C5 reads,
but this card does not certify any replica topology. Confirm the connector's
supported read mode, source identity, permissions, schema, replay lag, and
required read consistency first. Keep lag visible in freshness reporting.
If a C1 stream and snapshot use different endpoints, prove position alignment
and retention across both; do not assume a replica snapshot matches the
primary's change position.

For PostgreSQL, include standby recovery conflicts and replay lag in the
trial. For MySQL, verify the selected endpoint actually exposes the required
binlog/GTID history before choosing C1. For Oracle, have the DBA confirm standby
read availability and applicable licensing before planning it. None of these
paths authorizes product writes to the source or automatic source reconfiguration.

Adaptive throttling and short-transaction mitigations remain unmeasured in this
T10 attempt. Advance them only after an actual over-budget phase identifies the
next mitigation, followed by unit tests, full-scale T1/T2, and a new T10 run.
