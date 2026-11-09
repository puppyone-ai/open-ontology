# T10-live-3: sustained-rate attempt

## Result and scope

S6-H-02 remains **not done**. No engine has a valid D24 baseline; no product
phase, C5 policy comparison, or product mitigation is accepted. Runs took
place on 2026-11-08 America/Los_Angeles (2026-11-09 UTC in run IDs).
The missing `T10-live.md` was read from commit `e51c684d2`; its steps 1-5
match the committed resume card. No task card was changed.

Oracle 23, MySQL 8.4, PostgreSQL 17 were attempted in order. Oracle was then
retried to validate harness fixes and diagnose its connection failures.
All sources had verified 4 CPU / 8 GiB limits and 2,000,000 bulk rows
(T1 row scale, the narrow T3/T4 schema, not T1 type coverage);
source and target images were native arm64. Targets were unconstrained.
The unchanged workload uses 64 open-loop workers, the 60/30/10 mix,
60-second settling trials and 30% decrements (six-trial cap). All eight
attempts passed the quiet-host gate: eight pre-existing containers, with
per-container peak CPU between 0.06% and 1.05% across runs. These are not
aggregate CPU peaks. No unrelated names or inspection data were recorded.

## Baseline measurements

Every foreground error below is `scheduler_queue_full`. No product reader
was running. The extra 30% step was taken once for each failed ten-minute
baseline, exactly as requested; both retries still failed. No queue sample
was dropped, no threshold changed, and no additional rate reduction was
silently substituted. Phase deltas against these baselines would be invalid.

| Engine | Attempt | Offered requests/s | p50 / p95 / p99 ms | Achieved requests/s | Queue errors | Arrival / observation seconds | D24 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| mysql 8.4.7 | Settled rate | 7250.557 | 0.118 / 209.098 / 7609.652 | 3611.677 | 2,181,217 | 600 / 604.811 | Invalid baseline |
| mysql 8.4.7 | Additional 30% reduction | 5075.390 | 0.313 / 104.032 / 6567.711 | 3820.281 | 750,954 | 600 / 603.723 | Invalid baseline |
| postgres 17.11 | Settled rate | 6790.665 | 0.295 / 0.663 / 39.207 | 6769.950 | 12,427 | 600 / 604.033 | Invalid baseline |
| postgres 17.11 | Additional 30% reduction | 4753.466 | 0.320 / 0.521 / 5.453 | 4746.783 | 4,007 | 600 / 603.320 | Invalid baseline |

MySQL passed its first 60-second trial at 7,250.557 requests/s. PostgreSQL
had 178 queue errors at 9,700.950, then zero at 6,790.665 requests/s.
MySQL's reduced-rate run first rejected arrivals in the 290-300 s window;
PostgreSQL's reduced-rate rejections occurred in the 20-30 and 530-540 s
windows. A short error-free settling trial therefore did not establish an
error-free ten-minute rate. Product throttling cannot fix these failures.

### Required engine x phase coverage

The baseline values are the additional-30%-reduction attempts. Oracle has
no ten-minute baseline. All missing values mean unmeasured, never zero.
C5 idle/changed uses period 24; C5 full idle/changed uses period 1.

| Engine | Phase | p50 / p95 / p99 ms | Achieved requests/s | Errors | Duration | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| oracle | baseline | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| oracle | initial | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| oracle | initial_duty100 | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| oracle | c1 | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| oracle | c5_idle | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| oracle | c5_changed | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| oracle | c5_full_idle | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| oracle | c5_full_changed | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| mysql | baseline | 0.313 / 104.032 / 6567.711 | 3820.281 | 750,954 | 600 s | Invalid baseline |
| mysql | initial | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| mysql | initial_duty100 | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| mysql | c1 | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| mysql | c5_idle | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| mysql | c5_changed | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| mysql | c5_full_idle | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| mysql | c5_full_changed | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| postgres | baseline | 0.320 / 0.521 / 5.453 | 4746.783 | 4,007 | 600 s | Invalid baseline |
| postgres | initial | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| postgres | initial_duty100 | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| postgres | c1 | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| postgres | c5_idle | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| postgres | c5_changed | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| postgres | c5_full_idle | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |
| postgres | c5_full_changed | Not measured | Not measured | Not measured | Not measured | Blocked by calibration/baseline |

### Side effects

| Baseline | CPU peak | Cumulative block IO read / write peak | Source counter peaks |
| --- | --- | --- | --- |
| mysql, settled | 233.10% | 31.8 MB / 17.300 GB | History list / purge backlog 513; Threads_running 66 |
| mysql, lower30 | 92.15% | 31.7 MB / 16.200 GB | History list / purge backlog 426; Threads_running 66 |
| postgres, settled | 72.33% | 0.0 MB / 18.900 GB | Reader xmin age 0; dead tuples 1074; autovacuum count 13; slot WAL 0 B |
| postgres, lower30 | 62.73% | 0.0 MB / 10.500 GB | Reader xmin age 0; dead tuples 1087; autovacuum count 10; slot WAL 0 B |

All four baseline metric streams completed without sampler errors. Block IO
is Docker's rounded cumulative counter, not per-phase IO. History list is
purge backlog, not seconds of lag. Autovacuum counts include earlier fixture
activity. No product reader or replication slot was active, so zero reader
xmin/WAL values do not certify product behavior. Oracle undo, retention,
ORA-01555, and LogMiner CPU phase peaks were **not measured**.

## Oracle calibration and scoped diagnosis

The original attempt failed during settling, retaining only `RuntimeError`.
It cannot retrospectively identify the cause. The cleanup fix closes each
trial's worker connections instead of retaining up to six pools. Unit tests
prove the lifecycle fix; the next live attempt still failed every trial.
Safe error-code capture then established this result (60 seconds per row):

| Trial | Offered requests/s | Arrivals | Errors | Error class/code |
| --- | --- | --- | --- | --- |
| 1 | 10959.235 | 657,555 | 301 | scheduler_queue_full |
| 2 | 7671.465 | 460,288 | 1,303 | OperationalError:DPY-6005 |
| 3 | 5370.025 | 322,202 | 878 | OperationalError:DPY-6005 |
| 4 | 3759.018 | 225,542 | 558 | OperationalError:DPY-6005 |
| 5 | 2631.312 | 157,879 | 386 | OperationalError:DPY-6005 |
| 6 | 1841.919 | 110,516 | 198 | OperationalError:DPY-6005 |

A separate diagnostic run preserved only numeric driver/OS codes and queried
`V$RESOURCE_LIMIT` through the fixture admin connection between trials. It
found 4,551 connection failures with `DPY-6005`, `DPY-6000`, `ORA-12516`.
Its sampled process utilization was 136-141 (high-water 142, limit 200);
session utilization 160-166 (high-water 227, limit 322). These snapshots do
not prove resource-limit exhaustion or the cause of listener admission
failure. The diagnostic's extra admin observations are disclosed and are not
production-impact phase measurements. No listener or source limits changed.

## Changes, mitigations and correctness gates

- `c2a8ab140`: close connections after each settling trial, including failure;
  preserve failed trial counts. Rate selection, duration and worker count
  remain unchanged. This fixes a harness resource-lifecycle defect, not a
  connector performance problem; Oracle still fails after it.
- `b553fbb3d`: retain only strictly allowlisted Oracle numeric error codes;
  arbitrary driver text remains redacted. No budget or error was relaxed.
- Existing periodic C5 full verification remains at 24. Its period-1
  comparison did not run. There are no valid before/after product-mitigation
  numbers. Adaptive throttle, short transactions and Oracle C1 fallback were
  not changed because no measured product phase exceeded D24.
- Full-scale live T1/T2 was not run: no connector mitigation was introduced.
  Offline harness tests below are not database correctness acceptance.

## Reproduction and validation

Raw outputs are under ignored `evidence/`; validation run ID is
`t10-live3-validation`. The runtime interpreter is the worktree-local venv
from `t10-live-preflight-20261108`. Each normal attempt used:

```sh
python tools/oo-test/t10.py --engine ENGINE --version VERSION \
  --phases baseline,initial,initial_duty100,c1,c5 --minutes 10 \
  --rate auto --cpus 4 --memory 8g --evidence evidence/NEW-RUN \
  --binary target/release/oo-load
```

The lower30 attempts substituted `--rate 5075.390110570812` (MySQL) and
`--rate 4753.465652362397` (PostgreSQL). Scripts, invocation logs, diagnostics,
and explicit retry reasons are retained in the validation run directory.

- `cargo build -p oo-cli --locked --target-dir target`: pass.
- `cargo build --manifest-path tools/oo-load/Cargo.toml --release --locked
  --features mysql-binlog --target-dir target`: pass; existing unused-variable
  warning in `src/documents.rs` remains.
- `cargo check --workspace --locked --target-dir target`: pass before each
  code commit. No Rust crate, dependency, lockfile or default changed.
- `python3 -m unittest discover -s tools/oo-test -p test_t10.py -v`:
  42 initial, 45 after cleanup, 46 after safe-code capture; all pass.
  Initial new-test failures (including a missing mock import) are preserved
  in `settle-regression-before.log`, not overwritten.
- `PYTHONPATH=tools/oo-test python3 -m unittest test_t1 test_t2 test_t3t4
  test_oracle_t1 -v`: 39 pass. No pre-existing failures encountered.
- `python3 -m py_compile tools/oo-test/t10.py tools/oo-test/test_t10.py`: pass.
- `python3 specs/scripts/check_spec.py`, `check_plan.py`, and
  `check_v02_plan.py`: pass (document checks, not D24 acceptance).

### Immutable summary hashes

| Run ID | summary.json SHA-256 |
| --- | --- |
| `t10-live3-mysql-8.4-20261109` | `e9c01708731b484b0f990e78b46bf50e00daf9ddb4ab9b7abdeb969e7a399469` |
| `t10-live3-mysql-8.4-20261109-lower30` | `c7180ed4238c8da0ddc956e3fda2eff951a7fcd429fa5ca48a2c5000d894fa4a` |
| `t10-live3-oracle-23-20261109` | `ee7ca34ee059a309cfc9e7a8b86f70d0a85ff120c3bb0be4ee39ceb26b0e2b2e` |
| `t10-live3-oracle-23-codes-20261109` | `938103327d266b96b54c6e98511baa82c98f595046e304c741fc10556e84954b` |
| `t10-live3-oracle-23-diagnostic-20261109` | `b10a46e6adba56d9b740d7e2bc6e01c7a64ddcf77e0a065b21c065adee5e1a0a` |
| `t10-live3-oracle-23-fixed-20261109` | `1c7a2c6001a10a44d4607c0f6af15726c8b25015a6b4282a569e9713c143226e` |
| `t10-live3-postgres-17-20261109` | `bebeb738e2aba9bf78bac6bff8e7df47d6e8a7e7569969c05191b912311c5def` |
| `t10-live3-postgres-17-20261109-lower30` | `02f8c4dcbe64fca6794abccd4a3a4ad696e0ab5466025d6dd1ec447acb812992` |

## Residuals and operator guidance

Do not deploy these offered rates as capacity recommendations. Retain safe
product defaults, bounded C5 full verification and low-load windows.
Before a new run, resolve the Oracle fixture's listener admission failures
and obtain error-free ten-minute foreground baselines for MySQL/PostgreSQL.
Any further change to calibration, workers, queue capacity or rate policy
must be explicit; this attempt did not expand the authorized extra step.
Then measure all phases and both C5 policies before applying mitigations in
order, with unit tests and full-scale T1/T2 on each affected engine.
No numeric undo/vacuum/binlog/WAL retention guarantee can be derived here;
use measured production generation rate and outage/read duration with
DBA-approved headroom. Replica reads need separate alignment/lag validation.

Every attempt reports empty cleanup failures. Final label-scoped inventory
is zero containers, volumes and networks; no unrelated resource was changed.
The final audit scanned 15,268 new evidence files against unrelated container
names obtained in memory from Docker stats: zero matching files. Names were
not retained in the audit output.
No shared-file patch is requested and Cargo.lock is unchanged. Guide/support
Production impact pages are within this card's explicit documentation scope.
