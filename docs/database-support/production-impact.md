# Production impact

No live D24 budget is verified for Oracle 23, MySQL 8.4, or PostgreSQL 17 by
`t10-live-preflight-20261108`. The host preflight was blocked; see the
[measurement status and settings](../guide/production-impact.md). Do not
interpret missing latency, error, retention, or duration values as zero.

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
