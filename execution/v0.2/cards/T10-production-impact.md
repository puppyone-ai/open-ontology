## Card T10 — production impact of the initial load, C1 and C5 (S6-H-02; D24)

Goal: measure, on a QUIET host (no other lanes running), how much the product's reads disturb a production
source's normal workload, and land mitigations when the D24 budget is exceeded. Priority order: Oracle 23,
MySQL 8.4, PostgreSQL 17 (then other versions only if time remains).

Budget (D24): with the product's default settings, foreground p95 latency ≤ +20 %, p99 ≤ +50 %, throughput
≤ −5 % versus baseline, zero foreground errors; side effects reported with numbers.

Harness (new `tools/oo-test/t10.py`, reuse t1/t2/t3t4 fixtures and engine startup):
- Source container with fixed CPU/memory limits (record them), dataset at full T1 scale or larger.
- Foreground: an OPEN-LOOP OLTP workload at a fixed arrival rate sized to ~50–60 % of the source's measured
  capacity (calibrate first), mixing point SELECT by key, UPDATE of a hot row set, INSERT; record per-request
  latency (p50/p95/p99), achieved rate, errors, in 10 s windows. Same workload for every phase.
- Phases per engine: (a) baseline 10 min; (b) initial full load (fresh target) to completion;
  (c) C1 steady state 10 min with the foreground writing; (d) C5 cycles (one idle, one with changes)
  under lite defaults; repeat (b) with duty_cycle 100 % to show the tradeoff.
- Source-side metrics each 10 s: container CPU and block IO; PostgreSQL backend_xmin age of the product
  session, n_dead_tup growth on the hot table, autovacuum runs, replication slot retained WAL bytes;
  MySQL `Innodb_history_list_length`, Threads_running, purge lag; Oracle undo usage (V$UNDOSTAT,
  TUNED_UNDORETENTION), ORA-01555 occurrences, LogMiner session CPU (V$SESSION / V$SESSTAT).
- Report per engine × phase: deltas vs baseline against the budget, side-effect peaks, durations.

If a phase exceeds the budget, implement mitigations (one at a time, re-measure each):
1. C5: do not full-read every cycle by default; read only ranges whose digest changed (the engine already
   supports layered digests; lite sets `full_read_every: 1` in deploy/lite/scheduler.py); keep a bounded
   periodic full verification.
2. Adaptive throttling in the read loop: back off when source latency or active sessions rise (PG
   pg_stat_activity active count, MySQL Threads_running — S1-C-02 already pauses on it, check it is on by
   default — Oracle active sessions), not only a fixed duty cycle.
3. Initial load without one long snapshot where engine holds hurt (PG xmin, MySQL history list): per-chunk
   short transactions, made consistent by the existing C5 correction before publication (T1 is already
   judged that way); keep the snapshot mode as an option.
4. Oracle: document undo-retention sizing from load duration; prefer C5 over LogMiner C1 when LogMiner CPU
   exceeds budget; document a standby / replica read setup for all three engines.
Every mitigation needs unit tests, T1/T2 correctness re-run on the affected engine, and docs (docs/guide,
docs/database-support). Commit on your branch as the owner, no agent trailers, `Plan: S6-H-02`.
Final report: tables per engine × phase (baseline vs phase, budget verdict, side effects), mitigations with
before/after numbers, remaining residuals with operator guidance.
