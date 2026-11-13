# Production impact

How much the product's reads disturb a production source's own workload, as
measured by T10 (S6-H-02, decision D24). Functional correctness (T1/T2) is a
separate acceptance: a correct load can still be too heavy, and a light one can
still be wrong.

**Status (2026-11-13): not signed off.** The final remeasurement fixed four
product defects and three measurement defects, and every Oracle 23 default
phase is within D24. PostgreSQL 17 C1 and idle C5 still exceed the p95/p99
budget by more than the baseline's own variability, Oracle's opt-in LogMiner
C1 cannot keep up with the test's write rate, and the MySQL 8.4 fixture stalls
on its own by several seconds, so its relative verdicts are not conclusive.
Engine-specific side effects, sizing and the replica option are in the
[database support notes](../database-support/production-impact.md).

## Budget and method

D24 budget, every phase against the same engine's own baseline: foreground
p95 at most +20%, p99 at most +50%, throughput at most -5%, zero foreground
errors. A product failure in a phase (a failed load, cycle or sync call) fails
the phase even when the foreground numbers are within budget.

`tools/oo-test/t10.py` runs one engine at a time. The source container is
limited to 4 CPUs / 8 GiB and holds 2,000,000 rows. The foreground is an
open-loop OLTP mix at a fixed arrival rate (60% point SELECT, 30% UPDATE of 20
hot rows, 10% INSERT) with 64 workers (32 on Oracle Free, whose listener
refuses more). The rate is half of the settled rate: burst capacity is
measured, the rate starting at 55% of it is lowered 30% at a time until a
60-second trial has no errors, and every phase offers half of that. Latency is
measured from each request's intended start, so queueing counts; a backlog
beyond 30 seconds of arrivals is an error.

Phases: a 10-minute baseline; the initial load into a fresh target at the
default 25% duty cycle, then at 100% (a loader measurement flag, not a product
setting); 10 minutes of C1 with the foreground writing; and four C5 cycles,
idle and with changes, at the lite period (`c5_full_read_every: 24`, which
transfers only changed ranges) and at period 1 (full transfer).

Three measurement defects were fixed during this run; each is recorded in the
run summary:

- The agent DB (target) sat on a volume of the same Docker disk as the source,
  so its write bursts stalled the source's commit fsyncs. PostgreSQL initial
  load p95 was 48.1 ms with the shared disk and 0.60 ms with the target on its
  own storage (`--target-tmpfs`), against a 0.49 ms baseline. No real
  deployment shares the source database's storage.
- Each phase connected its 64 writers inside its first measured second. On
  PostgreSQL that warm-up alone added about 12% to a 44-second window's p95
  while the 10-minute baseline diluted it, a bias only against short phases.
  Connections now open before the phase clock.
- PostgreSQL C1 failed `wal_retention_headroom_unproven` because the fixture
  set unlimited slot retention; the product rightly refuses that. The fixture
  now uses a bounded 16GB.

The host also stopped being quiet: unrelated containers used 12-17% of one CPU
in total for hours. The 5% guard refused every attempt, so the PostgreSQL and
supplementary MySQL runs used `--quiet-cpu-percent 25`; the observed load is in
each summary. Oracle and the first MySQL run met the 5% guard.

## Final measurement

Loader: `tools/oo-load` release with `mysql-binlog`. Oracle A used loader
SHA-256 `9f146f41...` (commit `4d254dfbe`); Oracle B used `2a20a1de...`
(`da95152f2`); MySQL and PostgreSQL used `00aea932...` (tree of `69b4cecf8`).
Raw evidence is in the worktree's ignored `evidence/` directory under the run
IDs: Oracle A `t10-final-oracle-23-r2`, Oracle B `t10-final-oracle-23-c1-r4`
(C1 remeasured after the LogMiner fixes, with its own baseline), MySQL auto
`t10-final-mysql-84-r1`, MySQL low `t10-final-mysql-84-low-r2`, PostgreSQL
`t10-final-postgres-17-r3`. Oracle A/B and MySQL auto ran before the
measurement fixes above (shared-disk target, lazy connections); both only
penalise phases, so their passes stand.

| Engine (run) | Phase | p50 / p95 / p99 ms | Req/s | Errors | Seconds | Δp95 / Δp99 / Δrate % | D24 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Oracle 23 A | baseline | 0.296 / 0.716 / 54.280 | 5734.8 | 0 | 600.0 | 0.0 / 0.0 / 0.00 | pass |
| Oracle 23 A | initial | 0.300 / 0.803 / 18.321 | 5734.7 | 0 | 27.1 | 12.1 / -66.2 / -0.00 | pass |
| Oracle 23 A | initial_duty100 | 0.440 / 776.660 / 1031.497 | 5734.5 | 0 | 8.8 | 108351.8 / 1800.3 / -0.01 | fail |
| Oracle 23 A | c1 | 0.296 / 0.682 / 1.458 | 5734.8 | 0 | 600.0 | -4.8 / -97.3 / 0.00 | fail (product: RuntimeError) |
| Oracle 23 A | c5_idle | 0.292 / 0.670 / 1.333 | 5734.8 | 0 | 196.3 | -6.4 / -97.5 / -0.00 | pass |
| Oracle 23 A | c5_changed | 0.297 / 0.695 / 3.653 | 5721.9 | 0 | 380.1 | -2.9 / -93.3 / -0.23 | pass |
| Oracle 23 A | c5_full_idle | 0.294 / 0.689 / 5.103 | 5734.8 | 0 | 201.2 | -3.8 / -90.6 / -0.00 | pass |
| Oracle 23 A | c5_full_changed | 0.295 / 0.682 / 2.659 | 5734.8 | 0 | 372.0 | -4.7 / -95.1 / -0.00 | pass |
| Oracle 23 B | baseline | 0.288 / 0.713 / 25.345 | 5434.7 | 0 | 600.0 | 0.0 / 0.0 / 0.00 | pass |
| Oracle 23 B | c1 | 0.297 / 0.747 / 5.854 | 5434.7 | 0 | 600.0 | 4.8 / -76.9 / 0.00 | fail (product: RuntimeError) |
| MySQL 8.4 auto | baseline | 2094.273 / 7977.123 / 10077.275 | 3495.4 | 0 | 600.0 | 0.0 / 0.0 / 0.00 | pass |
| MySQL 8.4 auto | initial | 0.398 / 759.245 / 1314.693 | 3545.1 | 0 | 22.4 | -90.5 / -87.0 / 1.42 | pass |
| MySQL 8.4 auto | initial_duty100 | 214.345 / 7706.802 / 8425.838 | 3544.9 | 0 | 27.3 | -3.4 / -16.4 / 1.42 | pass |
| MySQL 8.4 auto | c1 | 2013.105 / 7796.950 / 9660.680 | 3536.5 | 0 | 600.0 | -2.3 / -4.1 / 1.18 | pass |
| MySQL 8.4 auto | c5_idle | 0.326 / 17.278 / 101.671 | 3545.0 | 0 | 34.8 | -99.8 / -99.0 / 1.42 | pass |
| MySQL 8.4 auto | c5_changed | 0.369 / 6.651 / 81.962 | 3545.1 | 0 | 46.2 | -99.9 / -99.2 / 1.42 | pass |
| MySQL 8.4 auto | c5_full_idle | 0.364 / 3.147 / 75.739 | 3545.1 | 0 | 32.7 | -100.0 / -99.2 / 1.42 | pass |
| MySQL 8.4 auto | c5_full_changed | 0.323 / 2.715 / 64.714 | 3545.1 | 0 | 46.8 | -100.0 / -99.4 / 1.42 | pass |
| MySQL 8.4 low | baseline | 0.677 / 3283.893 / 5548.401 | 1772.6 | 0 | 600.0 | 0.0 / 0.0 / 0.00 | pass |
| MySQL 8.4 low | initial | 0.445 / 160.497 / 800.936 | 1772.5 | 0 | 19.1 | -95.1 / -85.6 / -0.00 | pass |
| MySQL 8.4 low | initial_duty100 | 0.752 / 1667.682 / 2136.695 | 1772.5 | 0 | 14.2 | -49.2 / -61.5 / -0.00 | pass |
| MySQL 8.4 low | c1 | 0.752 / 4763.288 / 6325.181 | 1772.6 | 0 | 600.0 | 45.1 / 14.0 / 0.00 | fail |
| MySQL 8.4 low | c5_idle | 0.464 / 1.538 / 400.190 | 1772.5 | 0 | 32.2 | -100.0 / -92.8 / -0.00 | pass |
| MySQL 8.4 low | c5_changed | 0.459 / 1.155 / 4.067 | 1772.5 | 0 | 37.6 | -100.0 / -99.9 / -0.00 | pass |
| MySQL 8.4 low | c5_full_idle | 0.510 / 1.518 / 8.227 | 1772.5 | 0 | 32.6 | -100.0 / -99.9 / -0.00 | pass |
| MySQL 8.4 low | c5_full_changed | 0.477 / 1.137 / 2.974 | 1772.5 | 0 | 37.9 | -100.0 / -99.9 / -0.00 | pass |
| PostgreSQL 17 | baseline | 0.294 / 0.445 / 0.778 | 4697.1 | 0 | 600.0 | 0.0 / 0.0 / 0.00 | pass |
| PostgreSQL 17 | initial | 0.297 / 0.465 / 1.084 | 4697.1 | 0 | 43.9 | 4.5 / 39.3 / -0.00 | pass |
| PostgreSQL 17 | initial_duty100 | 0.313 / 0.541 / 1.557 | 4697.0 | 0 | 12.8 | 21.5 / 100.1 / -0.00 | fail |
| PostgreSQL 17 | c1 | 0.318 / 0.504 / 1.786 | 4697.1 | 0 | 600.0 | 13.0 / 129.5 / 0.00 | fail |
| PostgreSQL 17 | c5_idle | 0.309 / 1.647 / 44.278 | 4697.1 | 0 | 51.5 | 269.8 / 5588.9 / -0.00 | fail |
| PostgreSQL 17 | c5_changed | 0.301 / 0.476 / 0.924 | 4697.0 | 0 | 72.9 | 7.0 / 18.7 / -0.00 | pass |
| PostgreSQL 17 | c5_full_idle | 0.306 / 0.513 / 2.370 | 4697.1 | 0 | 51.2 | 15.2 / 204.5 / -0.00 | fail |
| PostgreSQL 17 | c5_full_changed | 0.303 / 0.480 / 0.849 | 4697.0 | 0 | 74.7 | 7.7 / 9.1 / -0.00 | pass |

Verdicts are the D24 rule as implemented: `fail (product: ...)` is a failed product call with the foreground numbers shown for context. Throughput deltas above zero mean the phase completed slightly more requests than the baseline in its window. MySQL low offers half of the automatic rate (1772.6 requests/s) with the measurement fixes applied.

### How much of a delta is noise

Short phases are compared with a 10-minute baseline. The same test applied to
50-second slices of each baseline alone, with no product running, shows how
often D24 fails by chance:

| Baseline | 50 s slices | Fail p95 +20% | Fail p99 +50% | Largest slice delta p95 / p99 |
| --- | --- | --- | --- | --- |
| Oracle 23 A | 56 | 8 | 13 | +37968% / +1957% |
| MySQL 8.4 auto | 56 | 5 | 0 | +25% / +13% |
| MySQL 8.4 low | 56 | 20 | 0 | +73% / +19% |
| PostgreSQL 17 | 56 | 0 | 3 | +4% / +91% |

Oracle's baseline has isolated stalls of up to 1.4 s (median 10-second-window
p99 1.2 ms). The MySQL fixture stalls for 3-6 seconds at a time after a few
minutes even at half the automatic rate (InnoDB flushing on the fixture's
default redo capacity is the likely cause; the stalls occur with no product
running), so its baseline p50 was 2.1 s at the automatic rate and its relative
verdicts are not conclusive in either direction. PostgreSQL's baseline is
steady (p95 slices within +4%), so its C5-idle p95 (+270%) and its C1 and
full-idle C5 p99 (+130%, +205%) exceed anything the baseline produced alone.

## Defects found and fixed during the final measurement

Each fix has unit tests, a live test that fails on the previous loader and
passes on the fixed one, and fast L1 (T0/T2/T4/T7, plus T1/T8 on MySQL 8.4) on
the affected engines.

| Defect | Before | After |
| --- | --- | --- |
| Keyed C5 never moves its durable ranges, so an append tail accumulated every row inserted after the baseline until reading it exceeded the chunk budget; every later cycle failed (PostgreSQL with an unclassified `chunk_rows_replan` that failed the whole cycle, Oracle with `table_budget`). | `c5_growth` PG 17: the cycle after 6x-budget growth exits 1 and stays failed. Oracle T10 `c5_changed` / `c5_full_changed`: `sync_partial` (OTHER `table_budget`). | C5 splits a range whose discovery digest exceeds the budget through the engine's bounded splitter, gives the children fresh ids and a newer revision, reads them first, and publishes only a plan that keeps or exactly refines the parent ranges with every child staged. `c5_growth` passes on PG 17, MySQL 8.4 and Oracle 23; Oracle `c5_changed` / `c5_full_changed` fresh and within D24. |
| Oracle C5 proved a range by comparing digests taken before and after the payload, so a hot range under continuous updates was `source_changed` on every attempt. | Oracle T10 `c5_changed`: ITEMS stale `source_changed`. | C5 asks each keyset chunk for a range digest `AS OF` the payload's own read SCN, the proof PostgreSQL and MySQL bind in one transaction. ITEMS fresh every cycle. |
| Oracle LogMiner mined every commit from the cursor to the current SCN in one batch; a busy foreground made the first batch larger than the budget and no later call could progress. | `c1_backlog` (Oracle 23): `query_row_limit`, 0 batches. T10 C1: `logminer_batch_over_budget` on the first call. | The reader bisects the commit-SCN span to the first one that fits (an empty shorter span moves the lower bound), remembers it for the next batches and grows it after light batches; a single oversized commit still refuses. `c1_backlog` drains in 22 batches, every row equal. |
| Under the foreground write rate Oracle switched online redo logs between the reader's inventory and LogMiner's header check (ORA-01368). | T10 C1: first sync call failed. | The batch is mined again from the same cursor after a fresh inventory (ORA-01368/ORA-00310, up to three times); nothing of a failed batch was delivered. ORA-01291 still fails. |

Unknown Oracle server codes now keep their ORA number (no driver text), and C1
step failures print it as a separate `{"diagnostic":"source_error",...}` line,
which is how ORA-01368 was identified.

Blockers named by the remeasurement card: PostgreSQL changed-C5 latency and
backlog rejections are gone (changed C5 at both periods passes; no queue
rejections in any run); PostgreSQL C1 no longer loses its slot (bounded 16GB
retention, peak retained WAL 174 MB); MySQL C1 completes (T10 binds it at an
aligned position on the quiescent fixture; the bounded handoff repair keeps its
100,000-key cap) and changed C5 publishes fresh; Oracle initial loads complete
without replanning and C1/C5 preparation no longer times out.

## Residuals and operator guidance

- **PostgreSQL C5 (lite default).** Changed cycles pass, but idle cycles, which
  digest every range of a 2,000,000-row table, added 20-60 ms foreground stalls
  for single seconds (p95 0.445 to 1.647 ms in the final run; an earlier run
  on the same harness passed p95 and failed p99). Keep the 25% duty cycle and
  the period-24 default; schedule C5 for low-write windows on latency-sensitive
  primaries, or read from a replica, until a smaller per-operation read or an
  adaptive throttle is measured.
- **PostgreSQL C1.** Adds about 0.5 ms to p99 in steady state (p95 +13%, p99
  +130% including stalls) and at 25% duty applied about 870 row changes per
  second, so the test's 1,900 changes per second outran it: the 600-second
  phase took 1,294 seconds to drain. Size C1 to a measured change rate below
  that; otherwise use C5.
- **MySQL.** Every product call succeeded and every phase passed at the
  automatic rate and at half of it, except C1 at half rate (p95 +45%). That
  delta is inside the fixture's own variability (baseline-only slices reach
  +73%), so MySQL is neither signed off nor failed by this run: repeat it on a
  source whose foreground does not stall on its own (size
  `innodb_redo_log_capacity` for the write rate) before relying on it.
- **Oracle C1 (opt-in).** Keep `sync: auto` (C5), which passes every phase.
  `oo doctor` reports the `logminer_reader` requirement for every Oracle source
  with this measured limit: every LogMiner batch re-mines redo from the
  binding's fixed transaction floor, so its cost grows with redo generated
  since the baseline; at about 2,200 row changes per second it missed its
  30-second batch deadline within ten minutes, although the foreground stayed
  within D24 (p95 +4.8%). Opt in only for a change rate measured on the actual
  workload, size online redo logs so they do not switch every few seconds, and
  rebaseline periodically.
- **100% duty cycle.** Exists only as the loader's `--duty-cycle-percent`
  measurement flag; no stack or lite setting exposes it. It failed D24 on
  Oracle (p95 0.7 to 777 ms) and PostgreSQL (p95 +21.5%, p99 +100%). Keep 25%.
- **Storage.** Never put the agent DB's data on the source database's disk.
- Keep the other defaults: source-activity gates (PostgreSQL 16 active
  sessions / 30 s pause, MySQL 16 running threads / 120 s, Oracle 16 active
  sessions / 60 s) and `snapshot: chunk_consistent` for keyed PostgreSQL and
  MySQL tables. C5 idle cycles still compute source digests over the whole
  scope: change-only transfer is not change-only source IO.
- These are fixture measurements, not capacity certifications. Before enabling
  a binding on a busy primary, repeat the measurement with the actual workload
  and an absolute latency objective as well as the relative D24 budget.

## History

Earlier attempts are kept as evidence reports, not as current numbers:
[T10-live-4](../../tools/oo-test/T10-LIVE4.md) (no engine passed every phase),
[mitigation 1](../../tools/oo-test/T10-MITIGATE.md) (C5 period 24, half the
settled rate) and [mitigation 2](../../tools/oo-test/T10-MITIGATE2.md)
(chunk-consistent reads and their correctness evidence).
