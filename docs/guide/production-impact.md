# Production impact

Production-impact acceptance is separate from functional correctness. A passing
T1/T2 run, a successful build, or the lite C5 default is not a D24 measurement.

## Measurement status

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
Before another full run, establish sustainable open-loop generator capacity
with the same operation mix and metrics sampling, then obtain an error-free
ten-minute baseline. An explicit lower-rate diagnostic is not acceptance at
55% of measured source capacity. Product throttling cannot repair a failed
foreground-only baseline; do not change snapshot consistency on that evidence.

For engine-specific sizing and replica caveats, see
[production-impact support notes](../database-support/production-impact.md).
Commands and evidence details are in
[the T10 handoff](../../tools/oo-test/T10-VALIDATION.md).
