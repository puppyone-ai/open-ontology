# Production impact

Production-impact acceptance is separate from functional correctness. A passing
T1/T2 run, a successful build, or the lite C5 default is not a D24 measurement.

## Measurement status

The 2026-11-08 local T10 attempt is blocked by the quiet-host prerequisite:
eight unrelated containers were running. None was stopped or modified.
Evidence run: `t10-live-preflight-20261108`. No source fixture was started.

| Engine requested | Phases | p50 / p95 / p99 | Throughput | Errors | Side-effect peaks | Duration | D24 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Oracle 23 | All listed below | Not measured | Not measured | Not measured | Not measured | Not measured | Unverified |
| MySQL 8.4 | All listed below | Not measured | Not measured | Not measured | Not measured | Not measured | Unverified |
| PostgreSQL 17 | All listed below | Not measured | Not measured | Not measured | Not measured | Not measured | Unverified |

Required phases are baseline, initial, initial at 100% duty, C1, C5 idle/changed
with the lite period, and C5 idle/changed with period 1. The requested versions
are not runtime-verified versions. There are no before/after mitigation numbers.

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

Use a dedicated quiet host, not a host running customer services. The T10
preflight refuses any pre-existing running container; do not bypass the guard
or stop unrelated services. Build the CLI and release loader before calibration.
Run Oracle 23, MySQL 8.4, then PostgreSQL 17 sequentially with source limits
`--cpus 4 --memory 8g`; these are test controls, not production sizing advice.
Only the source is constrained; record the target and host configuration too.

Use 2,000,000 rows, automatic calibration (55% of measured capacity), a fixed
open-loop workload, and ten-minute baseline/C1 phases. Initial and C5 phases
run to completion. Compare each phase against the same baseline: p95 increase
at most 20%, p99 increase at most 50%, throughput decrease at most 5%, and zero
foreground errors. Missing samples or product failures are not passes.

For engine-specific sizing and replica caveats, see
[production-impact support notes](../database-support/production-impact.md).
Commands and evidence details are in
[the T10 handoff](../../tools/oo-test/T10-VALIDATION.md).
