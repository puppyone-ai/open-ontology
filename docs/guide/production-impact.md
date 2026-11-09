# Production impact

Production-impact acceptance is separate from functional correctness. A passing
T1/T2 run, a successful build, or the lite C5 default is not a D24 measurement.

## Measurement status: T10-live-3

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

## Re-measurement

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
