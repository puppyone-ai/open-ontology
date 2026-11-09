# Production impact

No live D24 budget is verified for Oracle 23, MySQL 8.4, or PostgreSQL 17.
The T10-live-3 sustained-rate attempt failed calibration for Oracle and the
ten-minute baseline for MySQL/PostgreSQL, even after their extra 30% step.
No initial/C1/C5 phase or mitigation comparison is available. See the
[current measured results](../guide/production-impact.md) and the
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
