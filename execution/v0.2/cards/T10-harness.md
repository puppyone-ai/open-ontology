## Card T10-harness — build the production-impact harness (S6-H-02; D24), no live runs yet

Branch `v0.2/t10` at `44c498144fcf2cceda5d5d759169472e3d4f32f8`. Commit as the owner (no agent trailers), trailer `Plan: S6-H-02`. Read
`execution/v0.2/cards/T10-production-impact.md` (the full method and budget) and the plan §5 T10 rows.

IMPORTANT: another lane is running fault suites on this Docker host. Do NOT start database containers or any live
run. Build and unit-test the harness only; live measurement is a later card.

Write `tools/oo-test/t10.py` (plus small helpers if needed), reusing the fixture/engine startup of t1.py, t2.py and
t3t4.py and the oo-load invocation used by t2.py:
- CLI: `--engine postgres|mysql|oracle --version V --phases baseline,initial,initial_duty100,c1,c5 --minutes N
  --rate auto|R --cpus C --memory M --evidence DIR`; source container started with fixed `--cpus/--memory`.
- Calibration: closed-loop ramp to find capacity; open-loop rate = 55 % of it unless `--rate` is given.
- Foreground workload per engine (point SELECT by key, UPDATE of a hot set, INSERT), open-loop with a scheduler
  that records intended vs actual start (coordinated-omission safe), per-request latency, errors; 10 s windows.
- Phases as in the card; the product run uses its defaults (and duty 100 for initial_duty100).
- Source metrics every 10 s: docker stats CPU/blkio; PG backend_xmin age of oo sessions, n_dead_tup on the hot
  table, autovacuum count, slot retained WAL (pg_wal_lsn_diff); MySQL Innodb_history_list_length, Threads_running;
  Oracle V$UNDOSTAT/TUNED_UNDORETENTION, ORA-01555 count, LogMiner session CPU.
- Output: per phase JSON (p50/p95/p99, achieved rate, errors, metric peaks) and a summary with budget verdicts
  (p95 ≤ +20 %, p99 ≤ +50 %, throughput ≤ −5 %, zero errors) and explicit verdict rows for the evidence index.
- Unit tests (`tools/oo-test/test_t10.py`): percentile and window math, open-loop scheduling, verdict logic, metric
  parsers with recorded sample outputs, CLI validation. Run them and `python3 -m py_compile`.
Report: commits, CLI, what each phase does, test counts, and an exact command list for the live run per engine.
