# Production impact

Production-impact acceptance is separate from functional correctness. A passing
T1/T2 run, a successful build, or the lite C5 default is not a D24 measurement.

## Pending mitigation: revised foreground rate

T10 automatic-rate runs now offer 50% of the settled sustainable rate after
the unchanged calibration and settling procedure. Receipts separately record
capacity, settled rate and used rate. Explicit rates remain overrides, not
measured utilization. This method change has 51 passing offline harness tests,
but no new live phase measurements; it is not a product snapshot mitigation.
S6-H-02 remains incomplete. See the [partial mitigation handoff](../../tools/oo-test/T10-MITIGATE.md)
for unresolved correctness, handoff-budget and remeasurement work.

## Current measurement: T10-live-4

**S6-H-02 remains incomplete; no engine has passed every D24 phase.**
Runs span November 8-9, 2026 local. Sources were limited to 4 CPUs / 8 GiB,
with 2,000,000 bulk rows; targets were unconstrained. The open-loop queue holds
30 seconds of arrivals, with 32 Oracle workers and 64 for the other engines.
Latency includes queueing; budget failures and missing measurements remain failures.

Oracle A is the original run; Oracle B is the fresh C5/C1 run, each compared
only with its own baseline. Oracle initial phases were interrupted incomplete
(1723.865 / 632.134 seconds) after repeated OTHER replanning; neither completed.
MySQL below is its one authorized extra-30%-reduction run, after 247801 queue
rejections invalidated the first baseline. Its error-free baseline still had
11.059-second p95, so a relative pass is not an absolute latency recommendation.
`prep` means preparation failed without a measured foreground phase.

| Engine | Phase | p50 / p95 / p99 ms | Requests/s | Foreground errors | Seconds | D24 |
| --- | --- | --- | --- | --- | --- | --- |
| Oracle A | baseline | 0.455 / 123.970 / 1596.343 | 11104.335 | 0 | 600.000 | pass |
| Oracle A | initial | 0.512 / 1145.836 / 2397.536 | 11104.335 | 0 | 1723.865 | fail |
| Oracle A | initial_duty100 | 0.692 / 994.735 / 1256.805 | 11103.746 | 0 | 632.134 | fail |
| Oracle B | baseline | 0.412 / 105.170 / 582.162 | 11233.818 | 0 | 600.000 | pass |
| Oracle B | c1 | unmeasured / unmeasured / unmeasured | unmeasured | unmeasured | prep 900.144 | fail |
| Oracle B | c5_idle | 0.388 / 51.646 / 95.159 | 11233.831 | 0 | 214.468 | pass |
| Oracle B | c5_changed | unmeasured / unmeasured / unmeasured | unmeasured | unmeasured | prep 900.015 | fail |
| Oracle B | c5_full_idle | 0.400 / 47.136 / 93.132 | 11233.826 | 0 | 225.108 | pass |
| Oracle B | c5_full_changed | unmeasured / unmeasured / unmeasured | unmeasured | unmeasured | prep 900.013 | fail |
| MySQL 8.4.7 | baseline | 2835.038 / 11059.425 / 12725.698 | 4753.821 | 0 | 600.000 | pass |
| MySQL 8.4.7 | initial | 0.658 / 158.342 / 392.462 | 4787.797 | 0 | 9.780 | pass |
| MySQL 8.4.7 | initial_duty100 | 0.438 / 8.986 / 51.891 | 4788.002 | 0 | 7.432 | pass |
| MySQL 8.4.7 | c1 | unmeasured / unmeasured / unmeasured | unmeasured | unmeasured | prep 7.162 | fail |
| MySQL 8.4.7 | c5_idle | 2277.176 / 7646.172 / 9354.183 | 4775.392 | 0 | 847.165 | pass |
| MySQL 8.4.7 | c5_changed | 302.169 / 6166.454 / 7355.519 | 4788.292 | 0 | 198.916 | fail |
| MySQL 8.4.7 | c5_full_idle | 2220.150 / 7611.021 / 9886.084 | 4788.295 | 0 | 729.998 | pass |
| MySQL 8.4.7 | c5_full_changed | 2547.420 / 7798.524 / 10392.377 | 4788.222 | 0 | 523.882 | fail |
| PostgreSQL 17.11 | baseline | 0.303 / 210.157 / 1600.430 | 8217.958 | 0 | 600.000 | pass |
| PostgreSQL 17.11 | initial | 0.341 / 4.500 / 186.494 | 8217.720 | 0 | 31.108 | pass |
| PostgreSQL 17.11 | initial_duty100 | 171.564 / 198.957 / 214.339 | 2770.392 | 0 | 0.277 | fail |
| PostgreSQL 17.11 | c1 | 0.309 / 40.557 / 1436.641 | 8217.958 | 0 | 600.000 | fail |
| PostgreSQL 17.11 | c5_idle | 0.373 / 28.279 / 1481.780 | 8217.912 | 0 | 197.945 | pass |
| PostgreSQL 17.11 | c5_changed | 1.059 / 29663.543 / 33136.613 | 7484.980 | 32688 | 380.813 | fail |
| PostgreSQL 17.11 | c5_full_idle | 0.366 / 1.760 / 35.745 | 8217.919 | 0 | 197.381 | pass |
| PostgreSQL 17.11 | c5_full_changed | 0.570 / 83009.804 / 88405.562 | 4939.877 | 3282002 | 1076.422 | fail |

C5 names without `full` use period 24; `full` uses period 1. Each has an
independent snapshot. The INSERT table grows across phases, so this sequential
comparison is not an isolated causal speedup. PostgreSQL changed C5 exceeded
D24 at both periods: p95 29.664 / 83.010 seconds, throughput 7484.980 / 4939.877
requests/s, and 32688 / 3282002 backlog rejections (period 24 / 1).

Oracle changed-C5 and C1 preparation each timed out at 900 seconds. MySQL C1
failed `mysql_repair_staging_budget`; both changed-C5 policies returned
`sync_partial`. PostgreSQL duty-100 was refused by the connector's 25% cap;
C1 reported `rebaseline_required_slot_invalidated`. Favorable foreground
latency cannot turn these product failures into passes.

Adaptive activity gates already exist in all three engines. No new product
mitigation or snapshot-consistency change was introduced, and full-scale live
T1/T2 was not run. Adaptive tuning, short-transaction correctness and their
remeasurement remain unfinished exits. Keep safe defaults and low-write windows;
use the engine-specific [operator guidance](../database-support/production-impact.md).

See the [full phase report](../../tools/oo-test/T10-LIVE4.md) for every run,
side-effect peaks, failure receipts, policy comparisons, commands and hashes.
Raw outputs are under the five `t10-live4-*` run IDs in that report and
`evidence/t10-live4-validation/`. All owned resources were removed.

## Previous measurement: T10-live-3

S6-H-02 / D24 remains unverified after the sustained-rate attempt on
2026-11-08 local (2026-11-09 UTC). Oracle 23.5.0.24.07 could not settle;
MySQL 8.4.7 and PostgreSQL 17.11 failed their ten-minute baselines, including
the required additional 30% rate reduction. No product phase was measured.

| Engine | Attempt | Offered requests/s | p50 / p95 / p99 ms | Achieved requests/s | Queue errors | Arrival / observation seconds | D24 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| mysql 8.4.7 | Settled rate | 7250.557 | 0.118 / 209.098 / 7609.652 | 3611.677 | 2,181,217 | 600 / 604.811 | Invalid baseline |
| mysql 8.4.7 | Additional 30% reduction | 5075.390 | 0.313 / 104.032 / 6567.711 | 3820.281 | 750,954 | 600 / 603.723 | Invalid baseline |
| postgres 17.11 | Settled rate | 6790.665 | 0.295 / 0.663 / 39.207 | 6769.950 | 12,427 | 600 / 604.033 | Invalid baseline |
| postgres 17.11 | Additional 30% reduction | 4753.466 | 0.320 / 0.521 / 5.453 | 4746.783 | 4,007 | 600 / 603.320 | Invalid baseline |

Oracle's six 60-second trials ranged from 10,959.235 down to 1,841.919
requests/s. The first had 301 queue rejections; later trials had 1,303, 878,
558, 386 and 198 `DPY-6005` connection errors. A separate fixture-only
numeric-code diagnostic identified `DPY-6000` / `ORA-12516`; sampled process
and session usage remained below their configured limits, so resource-limit
exhaustion is not established. Undo/retention/LogMiner phase peaks are absent,
not zero. Closing each trial's connections fixes a harness leak but did not
resolve these admission failures.

The 64-worker, 60-second, 30%-step method is unchanged. The source limits
were verified at 4 CPUs / 8 GiB with 2,000,000 bulk rows. Product defaults,
queue capacity and D24 gold values were not relaxed. The lite C5 period-24
versus period-1 comparison, adaptive mitigation and live T1/T2 remain unrun:
no valid baseline exists to attribute an over-budget phase to product reads.

See the [full run report](../../tools/oo-test/T10-LIVE3.md) for every engine
x phase, side-effect peaks, exact retry reasons, commands, summary hashes,
validation results and cleanup. Eight unrelated containers were observed
only via stats; none was modified. All owned resources were removed.

## Previous attempt: T10-live-2

The resumed 2026-11-08 local run passed the updated quiet-host gate, but
all three ten-minute baselines failed with load-generator queue rejections.
No D24 production-impact comparison is valid. These are measured failed
baselines, not zero-impact results or database error counts.

| Engine requested | Phases | p50 / p95 / p99 | Throughput | Errors | Side-effect peaks | Duration | D24 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Oracle 23.5.0.24.07 | Baseline | 0.386 / 42.641 / 95.339 ms | 11,327.92 requests/s | 49,862 queue rejections | CPU 195.24%; undo 100,769 blocks; retention 900 s; ORA-01555 0 | 600 s | Invalid baseline |
| MySQL 8.4.7 | Baseline | 0.241 / 127.395 / 1,111.951 ms | 3,548.20 requests/s | 2,021,433 queue rejections | CPU 232.45%; history list 472; running threads 34 | 600 s | Invalid baseline |
| PostgreSQL 17.11 | Baseline | 0.299 / 5.263 / 55.786 ms | 9,421.35 requests/s | 73,129 queue rejections | CPU 82.05%; xmin age 0; dead tuples 1,310; autovacuum count 11; slot WAL 0 bytes | 600 s | Invalid baseline |
| All three engines | initial / initial_duty100 / C1 / four C5 variants | Not measured after baseline refusal | Not measured | Not measured | Not measured | Not measured | Unverified |

Required phases are baseline, initial, initial at 100% duty, C1, C5 idle/changed
with the lite period, and C5 idle/changed with period 1. There are no valid
baseline-versus-product deltas or before/after product-mitigation numbers.
An earlier Oracle trial had 115,824 queue rejections and was interrupted during
initial loading; its partial load must not be treated as completion evidence.

Evidence runs: `t10-live2-oracle-corrected-20261109`,
`t10-live2-mysql-corrected-20261109`, and
`t10-live2-postgres-corrected-20261109`. The original failed Oracle trial is
`t10-live2-oracle23-20261109`; build, tests, diagnostics and summary hashes are
in `t10-live2-20261109`. Raw artifacts remain ignored and private.

The source limits were verified as 4 CPUs / 8 GiB; targets were unconstrained.
All three used native arm64 images, 2,000,000 bulk rows, and a macOS 27.0 arm64
host with 16 CPUs / 128 GiB RAM (Docker: 16 CPUs / 94.1 GiB).
The eight pre-existing containers were sampled only via Docker stats and were
not modified. Aggregate peak CPU at the three-sample preflight was 0.63%,
0.97%, and 0.42%, respectively. Every owned source and target was removed.

Calibration selected 55% of measured closed-loop capacity: 11,411.03,
6,919.01, and 9,543.24 requests/s, respectively. A calibration mix defect
was corrected by assigning a single operation sequence across workers, but
the corrected baselines still overflowed the bounded open-loop queue. Thus
closed-loop capacity did not establish a sustainable open-loop rate on this
host. The harness now records error classes and refuses product comparisons
after an invalid baseline. Queue rejection counts remain failures, not dropped
samples. Do not attribute them to connector reads, which had not started.

## Safe starting settings

- Keep product defaults until measured against the actual workload. In lite,
  `schedule.c5_full_read_every` defaults to 24; 1 requests full transfer every
  cycle. The source still computes digests across the tables every cycle:
  change-only transfer does not mean change-only source IO.
- Keep periodic full verification enabled. The scheduler also forces full
  verification on its first cycle; T10's period-24 comparison uses cycle 1
  after an explicit baseline to measure the non-periodic path.
- Do not promote T10's 100% duty comparison into a production recommendation.
  Run heavy initial loads and full verification in an agreed low-load window
  while continuing to observe foreground latency and errors.
- If any default phase exceeds D24, measure mitigations separately and in
  order: periodic rather than every-cycle full transfer; adaptive source
  activity throttling; short chunk transactions with C5 correction before
  publication; Oracle undo sizing and C5 instead of expensive LogMiner C1.
  Do not remove snapshot consistency without the correction and full-scale
  T1/T2 correctness revalidation.

## Historical T10-live-3 re-measurement procedure

Prefer a dedicated quiet host. The accepted T10 preflight samples pre-existing
containers three times and refuses aggregate peak CPU above 5%; it never
stops them. Do not bypass the guard or modify unrelated services. Build the
CLI and release loader before calibration.
Run Oracle 23, MySQL 8.4, then PostgreSQL 17 sequentially with source limits
`--cpus 4 --memory 8g`; these are test controls, not production sizing advice.
Only the source is constrained; record the target and host configuration too.

Use 2,000,000 rows, automatic calibration (55% of measured capacity), a fixed
open-loop workload, and ten-minute baseline/C1 phases. Initial and C5 phases
run to completion. Compare each phase against the same baseline: p95 increase
at most 20%, p99 increase at most 50%, throughput decrease at most 5%, and zero
foreground errors. Missing samples or product failures are not passes.
After burst calibration at 55%, use 60-second sustained trials with 30%
rate decrements and 64 open-loop workers, then obtain an error-free ten-minute
baseline. If that baseline errors, record it and retry at an additional 30%
reduction as authorized by T10-live-3. This attempt exhausted that extra
step for MySQL/PostgreSQL without a valid baseline. A settled or explicitly
reduced rate is not acceptance at 55% of measured source capacity. Product throttling cannot repair a failed
foreground-only baseline; do not change snapshot consistency on that evidence.

For engine-specific sizing and replica caveats, see
[production-impact support notes](../database-support/production-impact.md).
Commands and evidence details are in
[the T10 handoff](../../tools/oo-test/T10-VALIDATION.md).
