# T10-live-4: bounded-backlog live measurements

## Acceptance and residuals

S6-H-02 remains **not done**. These runs measure production impact, not T1/T2
correctness or a supported production capacity. No engine has passed every phase.
Run IDs use UTC; work spans November 8-9, 2026 in America/Los_Angeles.

### What changed

- `2a64a52f2`: aggregate ten-second windows in one pass, rather than rescanning
  the entire phase twice per window. Boundary/reference and traversal-count tests
  preserve results. The original Oracle run used the old implementation; subsequent
  processes use the optimized implementation. No workload or D24 rule changed.
- `990680fc1`: a C1/C5 preparation failure now yields an explicitly failed,
  unmeasured phase and does not prevent independent later phases. The first Oracle
  process and first MySQL process had already imported the earlier code. Subsequent
  runs use the fix. Exception text remains redacted.
- Product code, source init parameters, dependency locks and default policies did
  not change. Oracle used 32 workers without ORA-12516; other engines used 64.

### Failure evidence

Oracle's original default and duty-100 initial loads were interrupted by sending
SIGTERM only to the verified owned loader child after 1723.865 and 632.134 seconds.
They are **incomplete failed loads**, not completion timings. Target receipts showed
OTHER plan revisions 11 and 10 and thousands of repeated staged chunks; separate
read-only target observations are retained in the validation run. The first
interruption note overestimated 30 minutes; a separate correction records the
measured 28.731 minutes without overwriting the original. The subsequent quiescent
C1 snapshot hit its existing 900-second command timeout. Bulk/items receipts do
not prove the entire binding completed. A fresh Oracle run measures baseline,
C5 then C1 to avoid repeating the interrupted initial paths; it has its own
baseline and is not combined with the old baseline for verdicts.

MySQL's first baseline had 247801 backlog rejections. The one additional 30%
reduction used 4788.298497 requests/s and produced an error-free baseline, but its
p95 was 11.059 seconds. Thus relative-budget passes do not imply acceptable absolute
OLTP latency. C1 preparation failed with `mysql_repair_staging_budget`; both changed
C5 policies returned `sync_partial`. Those are product failures despite zero
foreground errors and favorable relative latency. No failed policy was relabeled
as successful.

PostgreSQL's 100% duty attempt failed immediately with `config fact=read_budget`;
its connector explicitly caps duty at 25% and tests that cap. This report does not
weaken that safety test to satisfy a benchmark override. C1 returned
`rebaseline_required_slot_invalidated`; retained WAL peaked at 305079376 bytes
with the fixture's 256 MiB slot-retention cap. The ten-minute foreground stream
continued, but it was not ten minutes of successful C1 synchronization.

### Mitigations and before/after evidence

The already-landed period-24 C5 policy was compared with period 1, without changing
it. PostgreSQL changed C5: p95 29663.543 versus 83009.804 ms; p99 33136.613 versus
88405.562 ms; throughput 7484.980 versus 4939.877 requests/s; backlog errors 32688
versus 3282002; durations 380.813 versus 1076.422 seconds. Both failed D24. Period 24
was less disruptive in this sequential trial, but this is not an isolated causal
speedup: the INSERT table grows across phases and the policies have independent
snapshots of different sizes. Idle C5 does not establish changed-C5 safety.

All three connectors already gate reads on source activity (PG/Oracle active
sessions, MySQL Threads_running); this is not a newly implemented mitigation.
No adaptive-throttle tuning or short-transaction replacement was introduced or
validated. In particular, PostgreSQL holds its exported snapshot through C5:
changed period-24 versus period-1 peak reader xmin age was 1119964 / 2121968,
and peak hot-table dead tuples 840325 / 1591492. Removing that snapshot requires
proving the C5 publication correction, not merely dropping BEGIN/COMMIT.
Full-scale T1/T2 and mitigation remeasurement remain required and were **not run**.
This is an explicit unfinished exit, not a waived correctness gate.

### Operator guidance

- Do not recommend the calibrated rates as production capacity. The baseline tails
  are large and drift substantially; repeat on a dedicated host with the actual
  workload and an absolute latency SLO as well as the relative D24 budget.
- Keep period-24 bounded full verification. Source digest scans still read the
  table; avoiding full transfer is not avoiding source IO. Schedule changed C5 and
  initial loads in low-write windows until the failed engine paths are resolved.
- PostgreSQL: alert on reader xmin, dead tuples and slot WAL. Size retained WAL
  from measured WAL generation times initial-load/outage/catch-up duration, with
  headroom. Do not disable vacuum or call the 256 MiB fixture cap production sizing.
  Resolve snapshot lifetime with correction-before-publication and full T1/T2.
- MySQL: monitor history list as purge backlog, not seconds of lag. Preserve
  binlog/GTID history through load and recovery; investigate C1 repair-budget and
  partial C5 failures before selecting those paths.
- Oracle: no ORA-01555 was observed in completed measurement samples, but that does
  not validate the interrupted initial load. Size undo from peak generation times
  longest consistent-read interval plus DBA-approved headroom; the sampled
  V$UNDOSTAT history totals are not a phase-local generation rate. Prefer evaluating
  C5 over LogMiner if C1 CPU is too costly; absent successful C1 samples cannot
  establish such a CPU comparison.
- Replica/standby reads remain a separately validated option for all three engines:
  prove source identity, permissions, replay lag, snapshot/log alignment and
  retention. Oracle standby availability/licensing needs DBA approval. Never hide
  replica lag or modify source settings through the read-only product account.

### Validation

Evidence run `t10-live4-validation` contains build/check logs, tests, invocation
scripts, interruption reasons and scoped diagnostics. Both required builds passed;
workspace locked checks passed before each harness commit. The T10 suite increased
from 47 to 50 passing tests; 39 related T1/T2/T3T4/Oracle offline tests passed.
The new aggregation scaling test intentionally fails against base `1dd47b264`
(200000 visits rather than 1000); it passes after the fix. This is a reproduced
harness defect, not a pre-existing failure exemption. No Rust crate was edited.

No shared-file patch or Cargo.lock change is requested. The PG duty cap and Oracle
replanning require dedicated fixes and correctness evidence; no untested exact
patch is presented as ready for integration.

## Final Oracle coverage and cleanup

The fresh Oracle run completed both idle C5 policies, but changed C5 period 24,
changed C5 period 1, and C1 preparation each hit the existing 900-second timeout.
They are explicitly unmeasured failed phases. No foreground errors occurred in
its measured baseline or idle C5 phases. Every run reported empty cleanup failures;
final lane-scoped counts are zero containers, volumes and networks. No unrelated
container was stopped, inspected beyond stats, or modified.

## Final validation audit

Final T10 tests: 50 passed. Related offline fixture tests: 39 passed.
`py_compile`, `git diff --check`, `cargo check --workspace --locked --target-dir target`, and `check_spec.py`, `check_plan.py`, `check_v02_plan.py` all passed.
No Rust crate changed, so crate formatting/testing was not applicable.
The name audit scanned 21287 new evidence files (17751752431 bytes) against
unrelated identities obtained only from Docker stats in memory: zero matches;
identities were not retained. Final label-scoped inventory: zero containers,
volumes and networks. Validation receipts are in `t10-live4-validation`.

## Reproduction

Build `cargo build -p oo-cli --locked --target-dir target` and
`cargo build --manifest-path tools/oo-load/Cargo.toml --release --locked
--features mysql-binlog --target-dir target`. Use the worktree-local evidence venv:

```sh
python tools/oo-test/t10.py --engine ENGINE --version VERSION \
  --phases baseline,initial,initial_duty100,c1,c5 --minutes 10 \
  --rate auto --cpus 4 --memory 8g --evidence evidence/NEW-RUN \
  --binary target/release/oo-load
```

The MySQL retry uses `--rate 4788.298496605081`. The fresh Oracle run uses
`--phases baseline,c5,c1 --rate auto`. Both invocations and exact run IDs are
retained in the validation run. Do not reuse an existing evidence directory.

## Measurements

Raw evidence remains git-ignored. Missing phases are unmeasured, not passes.
Latency includes intended-arrival queueing; the queue holds 30 seconds of arrivals.
Oracle uses 32 foreground workers; MySQL/PostgreSQL use 64.
All runs request 4 CPUs / 8 GiB source limits and 2,000,000 bulk rows.
Targets are unconstrained; the narrow fixture is not T1 type coverage.

## oracle 23.5.0.24.07

Run: `t10-live4-oracle-23-20261109T042307Z`.
Summary SHA-256: `a946088a2c2f4ad4d8228fe2b0db6fc91c85a5e52ef9d071619774afbe330b1a`.
Run errors: `['RuntimeError']`; cleanup failures: `[]`.
Source limits: `{'NanoCpus': 4000000000, 'Memory': 8589934592}`.
Calibration: `{"capacity": 20189.710465487955, "rate": 11104.340756018377, "samples": [{"concurrency": 1, "errors": 0, "rate": 3093.5599164754935, "seconds": 10.002069083973765}, {"concurrency": 2, "errors": 0, "rate": 7004.098413801792, "seconds": 10.001430000178516}, {"concurrency": 4, "errors": 0, "rate": 11095.49242456079, "seconds": 10.002530374797061}, {"concurrency": 8, "errors": 0, "rate": 15922.211053707568, "seconds": 10.005080290837213}, {"concurrency": 16, "errors": 0, "rate": 20189.710465487955, "seconds": 10.008365416899323}, {"concurrency": 32, "errors": 0, "rate": 13243.465545638659, "seconds": 10.127334083197638}], "settle": {"rate": 11104.340756018377, "trials": [{"error_counts": {}, "errors": 0, "rate": 11104.340756018377, "requests": 666261, "seconds": 60}]}}`.

| Phase | p50 / p95 / p99 ms | Requests/s | Errors | Duration / observation s | p95 / p99 / rate delta % | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| baseline | 0.455 / 123.970 / 1596.343 | 11104.335 | 0 | 600.000 / 605.477 | 0.000 / 0.000 / 0.000 | pass |
| initial | 0.512 / 1145.836 / 2397.536 | 11104.335 | 0 | 1723.865 / 1739.268 | 824.286 / 50.189 / 0.000 | fail |
| initial_duty100 | 0.692 / 994.735 / 1256.805 | 11103.746 | 0 | 632.134 / 637.367 | 702.401 / -21.270 / -0.005 | fail |
| c1 | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| c5_idle | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| c5_changed | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| c5_full_idle | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| c5_full_changed | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |

| Phase | Side-effect peaks | Product / metric errors |
| --- | --- | --- |
| baseline | block_read_bytes=26200000.000; block_write_bytes=21800000000.000; cpu_percent=266.040; logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=900.000; undo_blocks=106813.000 | `[]` / `[]` |
| initial | block_read_bytes=158000000.000; block_write_bytes=61400000000.000; cpu_percent=344.880; logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=985.000; undo_blocks=335919.000 | `['RuntimeError']` / `[]` |
| initial_duty100 | block_read_bytes=160000000.000; block_write_bytes=75200000000.000; cpu_percent=347.830; logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=900.000; undo_blocks=420990.000 | `['RuntimeError']` / `[]` |

Block IO is cumulative and rounded by Docker. Undo/autovacuum counters include fixture history.
History-list length is a purge backlog, not seconds of lag. LogMiner CPU samples can miss exited sessions.

## mysql 8.4.7

Run: `t10-live4-mysql-8.4-20261109T053746Z`.
Summary SHA-256: `5e4df76aab0f6309774aefa7ff66626645484051aceabc52a2a989672462150f`.
Run errors: `['invalid_baseline']`; cleanup failures: `[]`.
Source limits: `{'NanoCpus': 4000000000, 'Memory': 8589934592}`.
Calibration: `{"capacity": 12437.13895222099, "rate": 6840.426423721545, "samples": [{"concurrency": 1, "errors": 0, "rate": 1701.154390634788, "seconds": 10.003795125056058}, {"concurrency": 2, "errors": 0, "rate": 2889.6206167684345, "seconds": 10.001312917098403}, {"concurrency": 4, "errors": 0, "rate": 4888.94734274352, "seconds": 10.006653083022684}, {"concurrency": 8, "errors": 0, "rate": 8151.959695636109, "seconds": 10.003852207912132}, {"concurrency": 16, "errors": 0, "rate": 11977.755638995488, "seconds": 10.007884917082265}, {"concurrency": 32, "errors": 0, "rate": 12437.13895222099, "seconds": 10.017738040769473}], "settle": {"rate": 6840.426423721545, "trials": [{"error_counts": {}, "errors": 0, "rate": 6840.426423721545, "requests": 410426, "seconds": 60}]}}`.

| Phase | p50 / p95 / p99 ms | Requests/s | Errors | Duration / observation s | p95 / p99 / rate delta % | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| baseline | 0.866 / 41628.701 / 43431.068 | 6088.466 | 247801 | 600.000 / 640.007 | unmeasured / unmeasured / unmeasured | fail |
| initial | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| initial_duty100 | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| c1 | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| c5_idle | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| c5_changed | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| c5_full_idle | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| c5_full_changed | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |

| Phase | Side-effect peaks | Product / metric errors |
| --- | --- | --- |
| baseline | Innodb_history_list_length=621.000; Threads_running=66.000; block_read_bytes=31700000.000; block_write_bytes=27000000000.000; cpu_percent=371.770; purge_backlog_transactions=621.000 | `[]` / `[]` |

Block IO is cumulative and rounded by Docker. Undo/autovacuum counters include fixture history.
History-list length is a purge backlog, not seconds of lag. LogMiner CPU samples can miss exited sessions.

## postgres 17.11

Run: `t10-live4-postgres-17-20261109T055232Z`.
Summary SHA-256: `5361df7c432d7c30e7bb408625e40afff9fccb9689f4855cbacbd6e6f492b70d`.
Run errors: `[]`; cleanup failures: `[]`.
Source limits: `{'NanoCpus': 4000000000, 'Memory': 8589934592}`.
Calibration: `{"capacity": 14941.744645588393, "rate": 8217.959555073618, "samples": [{"concurrency": 1, "errors": 0, "rate": 3924.2745378405434, "seconds": 10.000319707905874}, {"concurrency": 2, "errors": 0, "rate": 9162.701448966722, "seconds": 10.000325832981616}, {"concurrency": 4, "errors": 0, "rate": 12938.50563727727, "seconds": 10.00045937509276}, {"concurrency": 8, "errors": 0, "rate": 14941.744645588393, "seconds": 10.000840165885165}, {"concurrency": 16, "errors": 0, "rate": 13059.555500858056, "seconds": 10.001412375131622}], "settle": {"rate": 8217.959555073618, "trials": [{"error_counts": {}, "errors": 0, "rate": 8217.959555073618, "requests": 493078, "seconds": 60}]}}`.

| Phase | p50 / p95 / p99 ms | Requests/s | Errors | Duration / observation s | p95 / p99 / rate delta % | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| baseline | 0.303 / 210.157 / 1600.430 | 8217.958 | 0 | 600.000 / 604.185 | 0.000 / 0.000 / 0.000 | pass |
| initial | 0.341 / 4.500 / 186.494 | 8217.720 | 0 | 31.108 / 31.289 | -97.859 / -88.347 / -0.003 | pass |
| initial_duty100 | 171.564 / 198.957 / 214.339 | 2770.392 | 0 | 0.277 / 1.107 | -5.329 / -86.607 / -66.289 | fail |
| c1 | 0.309 / 40.557 / 1436.641 | 8217.958 | 0 | 600.000 / 604.376 | -80.702 / -10.234 / 0.000 | fail |
| c5_idle | 0.373 / 28.279 / 1481.780 | 8217.912 | 0 | 197.945 / 199.144 | -86.544 / -7.414 / -0.001 | pass |
| c5_changed | 1.059 / 29663.543 / 33136.613 | 7484.980 | 32688 | 380.813 / 402.869 | 14014.947 / 1970.481 / -8.919 | fail |
| c5_full_idle | 0.366 / 1.760 / 35.745 | 8217.919 | 0 | 197.381 / 198.604 | -99.163 / -97.767 / -0.000 | pass |
| c5_full_changed | 0.570 / 83009.804 / 88405.562 | 4939.877 | 3282002 | 1076.422 / 1103.303 | 39398.955 / 5423.862 / -39.889 | fail |

| Phase | Side-effect peaks | Product / metric errors |
| --- | --- | --- |
| baseline | autovacuum_count=12.000; backend_xmin_age=0.000; block_read_bytes=0.000; block_write_bytes=19600000000.000; cpu_percent=101.750; n_dead_tup=1705.000; slot_retained_wal_bytes=0.000 | `[]` / `[]` |
| initial | autovacuum_count=13.000; backend_xmin_age=102158.000; block_read_bytes=0.000; block_write_bytes=20400000000.000; cpu_percent=107.730; n_dead_tup=76346.000; slot_retained_wal_bytes=0.000 | `[]` / `[]` |
| initial_duty100 | autovacuum_count=13.000; backend_xmin_age=0.000; block_read_bytes=0.000; block_write_bytes=20400000000.000; cpu_percent=63.030; n_dead_tup=2929.000; slot_retained_wal_bytes=0.000 | `['RuntimeError']` / `[]` |
| c1 | autovacuum_count=24.000; backend_xmin_age=8250.000; block_read_bytes=0.000; block_write_bytes=36000000000.000; cpu_percent=142.200; n_dead_tup=5028.000; slot_retained_wal_bytes=305079376.000 | `['RuntimeError']` / `[]` |
| c5_idle | autovacuum_count=28.000; backend_xmin_age=628178.000; block_read_bytes=0.000; block_write_bytes=40600000000.000; cpu_percent=294.660; n_dead_tup=472036.000; slot_retained_wal_bytes=0.000 | `[]` / `[]` |
| c5_changed | autovacuum_count=35.000; backend_xmin_age=1119964.000; block_read_bytes=0.000; block_write_bytes=48200000000.000; cpu_percent=413.020; n_dead_tup=840325.000; slot_retained_wal_bytes=0.000 | `[]` / `[]` |
| c5_full_idle | autovacuum_count=39.000; backend_xmin_age=628203.000; block_read_bytes=0.000; block_write_bytes=53000000000.000; cpu_percent=292.580; n_dead_tup=472160.000; slot_retained_wal_bytes=0.000 | `[]` / `[]` |
| c5_full_changed | autovacuum_count=58.000; backend_xmin_age=2121968.000; block_read_bytes=0.000; block_write_bytes=63100000000.000; cpu_percent=410.740; n_dead_tup=1591492.000; slot_retained_wal_bytes=0.000 | `[]` / `[]` |

Block IO is cumulative and rounded by Docker. Undo/autovacuum counters include fixture history.
History-list length is a purge backlog, not seconds of lag. LogMiner CPU samples can miss exited sessions.

## mysql 8.4.7

Run: `t10-live4-mysql-8.4-lower30-20261109T065136Z`.
Summary SHA-256: `e708005e9578a928068809801583a41574f1a0bde32cc647767be21225391637`.
Run errors: `[]`; cleanup failures: `[]`.
Source limits: `{'NanoCpus': 4000000000, 'Memory': 8589934592}`.
Calibration: `{"rate": 4788.298496605081, "skipped": "explicit rate"}`.

| Phase | p50 / p95 / p99 ms | Requests/s | Errors | Duration / observation s | p95 / p99 / rate delta % | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| baseline | 2835.038 / 11059.425 / 12725.698 | 4753.821 | 0 | 600.000 / 612.821 | 0.000 / 0.000 / 0.000 | pass |
| initial | 0.658 / 158.342 / 392.462 | 4787.797 | 0 | 9.780 / 9.844 | -98.568 / -96.916 / 0.715 | pass |
| initial_duty100 | 0.438 / 8.986 / 51.891 | 4788.002 | 0 | 7.432 / 7.486 | -99.919 / -99.592 / 0.719 | pass |
| c1 | unmeasured / unmeasured / unmeasured | unmeasured | unmeasured | 0.000 / 7.162 | unmeasured / unmeasured / unmeasured | fail |
| c5_idle | 2277.176 / 7646.172 / 9354.183 | 4775.392 | 0 | 847.165 / 855.686 | -30.863 / -26.494 / 0.454 | pass |
| c5_changed | 302.169 / 6166.454 / 7355.519 | 4788.292 | 0 | 198.916 / 199.612 | -44.243 / -42.199 / 0.725 | fail |
| c5_full_idle | 2220.150 / 7611.021 / 9886.084 | 4788.295 | 0 | 729.998 / 733.090 | -31.181 / -22.314 / 0.725 | pass |
| c5_full_changed | 2547.420 / 7798.524 / 10392.377 | 4788.222 | 0 | 523.882 / 525.599 | -29.485 / -18.336 / 0.724 | fail |

| Phase | Side-effect peaks | Product / metric errors |
| --- | --- | --- |
| baseline | Innodb_history_list_length=465.000; Threads_running=66.000; block_read_bytes=31800000.000; block_write_bytes=15100000000.000; cpu_percent=365.160; purge_backlog_transactions=465.000 | `[]` / `[]` |
| initial | Innodb_history_list_length=354.000; Threads_running=4.000; block_read_bytes=31800000.000; block_write_bytes=15100000000.000; cpu_percent=78.480; purge_backlog_transactions=354.000 | `[]` / `[]` |
| initial_duty100 | Innodb_history_list_length=304.000; Threads_running=7.000; block_read_bytes=31800000.000; block_write_bytes=15500000000.000; cpu_percent=77.860; purge_backlog_transactions=304.000 | `[]` / `[]` |
| c1 | unmeasured | `['RuntimeError']` / `[]` |
| c5_idle | Innodb_history_list_length=458.000; Threads_running=66.000; block_read_bytes=31900000.000; block_write_bytes=35600000000.000; cpu_percent=388.740; purge_backlog_transactions=458.000 | `[]` / `[]` |
| c5_changed | Innodb_history_list_length=420.000; Threads_running=66.000; block_read_bytes=154000000.000; block_write_bytes=40600000000.000; cpu_percent=376.320; purge_backlog_transactions=420.000 | `['RuntimeError']` / `[]` |
| c5_full_idle | Innodb_history_list_length=532.000; Threads_running=66.000; block_read_bytes=194000000.000; block_write_bytes=57900000000.000; cpu_percent=377.340; purge_backlog_transactions=532.000 | `[]` / `[]` |
| c5_full_changed | Innodb_history_list_length=623.000; Threads_running=66.000; block_read_bytes=434000000.000; block_write_bytes=69400000000.000; cpu_percent=377.830; purge_backlog_transactions=623.000 | `['RuntimeError']` / `[]` |

Block IO is cumulative and rounded by Docker. Undo/autovacuum counters include fixture history.
History-list length is a purge backlog, not seconds of lag. LogMiner CPU samples can miss exited sessions.

## oracle 23.5.0.24.07

Run: `t10-live4-oracle-23-remaining-20261109T074410Z`.
Summary SHA-256: `90e463f3f4b20fdcbf4e69611f4303754d27980c6509b63ee8a775b00ff8d0d2`.
Run errors: `[]`; cleanup failures: `[]`.
Source limits: `{'NanoCpus': 4000000000, 'Memory': 8589934592}`.
Calibration: `{"capacity": 20425.184192031364, "rate": 11233.85130561725, "samples": [{"concurrency": 1, "errors": 0, "rate": 3206.917582862513, "seconds": 10.003687083022669}, {"concurrency": 2, "errors": 0, "rate": 7107.480150904083, "seconds": 10.002701167017221}, {"concurrency": 4, "errors": 0, "rate": 11776.575929336124, "seconds": 10.003841584082693}, {"concurrency": 8, "errors": 0, "rate": 16316.830391441294, "seconds": 10.006784165976569}, {"concurrency": 16, "errors": 0, "rate": 20425.184192031364, "seconds": 10.012835041154176}, {"concurrency": 32, "errors": 0, "rate": 13625.974318100538, "seconds": 10.03686024993658}], "settle": {"rate": 11233.85130561725, "trials": [{"error_counts": {}, "errors": 0, "rate": 11233.85130561725, "requests": 674032, "seconds": 60}]}}`.

| Phase | p50 / p95 / p99 ms | Requests/s | Errors | Duration / observation s | p95 / p99 / rate delta % | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| baseline | 0.412 / 105.170 / 582.162 | 11233.818 | 0 | 600.000 / 606.146 | 0.000 / 0.000 / 0.000 | pass |
| initial | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| initial_duty100 | unmeasured | unmeasured | unmeasured | unmeasured | unmeasured | incomplete |
| c1 | unmeasured / unmeasured / unmeasured | unmeasured | unmeasured | 0.000 / 900.144 | unmeasured / unmeasured / unmeasured | fail |
| c5_idle | 0.388 / 51.646 / 95.159 | 11233.831 | 0 | 214.468 / 216.240 | -50.893 / -83.654 / 0.000 | pass |
| c5_changed | unmeasured / unmeasured / unmeasured | unmeasured | unmeasured | 0.000 / 900.015 | unmeasured / unmeasured / unmeasured | fail |
| c5_full_idle | 0.400 / 47.136 / 93.132 | 11233.826 | 0 | 225.108 / 227.041 | -55.181 / -84.002 / 0.000 | pass |
| c5_full_changed | unmeasured / unmeasured / unmeasured | unmeasured | unmeasured | 0.000 / 900.013 | unmeasured / unmeasured / unmeasured | fail |

| Phase | Side-effect peaks | Product / metric errors |
| --- | --- | --- |
| baseline | block_read_bytes=23200000.000; block_write_bytes=22400000000.000; cpu_percent=148.090; logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=900.000; undo_blocks=108118.000 | `[]` / `[]` |
| c5_idle | block_read_bytes=51400000.000; block_write_bytes=27900000000.000; cpu_percent=265.150; logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=900.000; undo_blocks=136571.000 | `[]` / `[]` |
| c5_changed | unmeasured | `['RuntimeError']` / `[]` |
| c5_full_idle | block_read_bytes=130000000.000; block_write_bytes=33600000000.000; cpu_percent=170.170; logminer_session_cpu_centiseconds=0.000; ora_01555_count=0.000; tuned_undoretention_seconds=1014.000; undo_blocks=166720.000 | `[]` / `[]` |
| c5_full_changed | unmeasured | `['RuntimeError']` / `[]` |
| c1 | unmeasured | `['RuntimeError']` / `[]` |

Block IO is cumulative and rounded by Docker. Undo/autovacuum counters include fixture history.
History-list length is a purge backlog, not seconds of lag. LogMiner CPU samples can miss exited sessions.

