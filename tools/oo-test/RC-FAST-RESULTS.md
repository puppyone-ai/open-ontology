# Release-candidate fast-suite run and T10 remeasurement

Plans `S6-H-01` (layered fast suite, `specs/test-acceleration-plan.md`,
[TA-01](TA-01-ACCEL.md)) and `S6-H-02` (T10, D24). Date 2026-11-13.
Release candidate: `v0.3/integrate` at `f6c8f8b2a` (v0.2/field-hardening plus
v0.3), then the fix commits below. Raw evidence is in this worktree's ignored
`evidence/` directory under the run IDs given here.

Host: one desktop Docker VM (16 CPUs, about 100 GB memory, about 59 GB free
disk at the start, lowest observed 55 GB); 14 pre-existing unrelated containers
stayed idle (T10 recorded their total peak CPU at 6.0% and 12.5% of one CPU).

## Verdict

- **Fast suite (L0, L1, L2, defect gate): every cell passed** after one genuine
  regression fix and four harness/corpus fixes; two cells (Oracle 23ai L1,
  MySQL 8.0 L2) needed a rerun after host-load noise (causes below).
- **QS-01..05: pass** on a package staged from this tree with a lite image
  rebuilt from this tree (`rc-qs2`; the first attempt's QS-05 saw the
  concurrent Oracle lanes' containers).
- **T10:** PostgreSQL 17 changed C5 passes; full-read changed C5 fails.
  MySQL 8.4 is still fixture-limited (its own baseline stalled 5.6 s). Not
  signed off.

## 1. Regression found and fixed

**oo-load with `mysql-binlog` did not compile** at `f6c8f8b2a`
(`tools/oo-load/src/handoff.rs` passed the `opts` argument that the merge
`a592e3d5b` had removed from `mysql_engine`). Every live layer and T10 run
oo-load built with `mysql-binlog`, so nothing live could run on the RC as
merged. The module is `#[cfg(feature = "mysql-binlog")]`, and L0's
`rust-oo-load` tested oo-load without that feature, so L0 could not catch it.
Fix `54a02d374`: the call drops the argument (the v0.3 `mysql_engine` resolves
the CA through `SecretStore::path`); L0 now runs
`cargo test --manifest-path tools/oo-load/Cargo.toml --bins --features mysql-binlog`
(passed, run `rc-l0-r`).

## 2. Harness and corpus fixes (not product)

All in `54a02d374` unless noted, each with unit tests:

| Finding | Cause | Fix |
| --- | --- | --- |
| Gate `oracle-worker-code`: error, not catch | The T10 round made Oracle driver errors keep their ORA number with the same statement `SourceError::new(kind, Some(fact), code)`, so the mutation's preimage matched twice | Mutation names the worker site (`.flatten();` plus the statement) |
| Gate `decimal256-carry`: error | Fix `127479529` is on this branch, so the v0.2 fix overlay's preimage is absent | Overlay skipped when every postimage is already in place and no preimage is left; a half-applied fix is still an error |
| L0 `py-lite` `test_up_provisions_configured_data_login_on_agent_db` failed | The test passes an empty environment, so the Compose resolver searched the process PATH and found this host's standalone `docker-compose` | Test passes a PATH without `docker-compose` |
| Scheduler and snapshot preflight caps (40 GB, two non-Oracle lane-ta containers, one Oracle) fixed in code | Host-specific | `OO_TEST_MAX_ORACLE`, `OO_TEST_MAX_OTHER_CONTAINERS`, `OO_TEST_DOCKER_CAP_GB`; defaults unchanged, the L1 plan records the caps |
| T10 had no way to remeasure single C5 sub-phases | - | `--c5-variants` (`edc0cd171`); the overall result requires only the selected sub-phases |

This run used `OO_TEST_MAX_ORACLE=2` (3 for the late Oracle reruns),
`OO_TEST_MAX_OTHER_CONTAINERS=24`, `OO_TEST_DOCKER_CAP_GB=70`.

## 3. Fast suite

Day 2026-11-13 rotation. Each L1 cell is one `oo-test fast --layer L1
--engines E --cells ...` process; seven such lanes ran in parallel (three
PostgreSQL, two MySQL, two Oracle), with L0, the defect gate and the lite
image build alongside. "Full" suites run on the version; "probe" suites are
covered by the T0 doctor probe today (equivalence classes, TA-01 section 4).

### L0 (`rc-l0`, rerun `rc-l0-r` at `edc0cd171`)

| Command | First run | Rerun |
| --- | --- | --- |
| rust-core-pipeline | passed 89 s | passed 37 s |
| rust-connectors | passed 36 s | passed 11 s |
| rust-cli | passed 36 s | passed 1 s |
| rust-oo-load (now with `mysql-binlog`) | passed 74 s | passed 0.4 s |
| rust-failpoints | passed 36 s | passed 12 s |
| py-oo-test | passed 8 s | passed 5 s |
| py-lite | failed (host compose, section 2) | passed 15 s |

### L1

| Engine | Version | Full | Probe | T0 | T1 | T8 | T7 | T2 | T4 | Verdict | Cell s | Run |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | ---: | --- |
| PostgreSQL | 13 | T1 T2 T4 T7 T8 | T0 | pass | pass | pass | pass | pass | pass | passed | 949 | `rc-p1` |
| PostgreSQL | 14 | T2 T4 T7 | T0 T1 T8 | pass | probe | probe | pass | pass | pass | passed | 525 | `rc-p1` |
| PostgreSQL | 15 | T1 T2 T4 T7 T8 | T0 | pass | pass | pass | pass | pass | pass | passed | 994 | `rc-p2` |
| PostgreSQL | 16 | T0 T2 T4 T7 | T1 T8 | pass | probe | probe | pass | pass | pass | passed | 526 | `rc-p2` |
| PostgreSQL | 17 | T2 T4 T7 | T0 T1 T8 | pass | probe | probe | pass | pass | pass | passed | 763 | `rc-p3` |
| PostgreSQL | 18 | all | - | pass | pass | pass | pass | pass | pass | passed | 756 | `rc-p3` |
| MySQL | 5.7 | all | - | pass | pass | pass | pass | pass | pass | passed | 859 | `rc-m1` |
| MySQL | 8.0 | all | - | pass | pass | pass | pass | pass | pass | passed | 507 | `rc-m1` |
| MySQL | 8.4 | all | - | pass | pass | pass | pass | pass | pass | passed | 709 | `rc-m2` |
| MySQL | 9.5 | all | - | pass | pass | pass | pass | pass | pass | passed | 507 | `rc-m2` |
| Oracle | 11g | all | - | pass | pass | pass | pass | pass | pass | passed | 2,530 | `rc-o1` |
| Oracle | 19c | all | - | pass | pass | pass | pass | pass | pass | passed | 2,544 | `rc-o3` |
| Oracle | 21c | T2 T4 T7 | T0 T1 T8 | pass | probe | probe | pass | pass | pass | passed | 1,136 | `rc-o2` |
| Oracle | 23ai | all | - | pass | pass | pass | pass | pass | pass | passed (rerun) | 557 | `rc-o2-r` |

Oracle 23ai first attempt (`rc-o2`, 538 s): failed with T4 passed. The
snapshot clone restart, next to the emulated 11g cell and the other lanes, ran its first statements
before the instance was open (`ORA-01034`, then `ORA-01109`), so the shared
cell aborted; T2 then failed its C1 part (C5 passed, the implicit-LogMiner
refusal held) under the same load. Harness timing, not product: rerun
`rc-o2-r` above. Oracle 19c's cell in `rc-o1` stayed `pending`
(`oracle_slot_occupied`) and ran in its own lane `rc-o3`.

### L2 (today's representatives: PostgreSQL 14, MySQL 8.0, Oracle 19c)

| Engine | Version | source_restart_mid_chunk | network_drop_mid_c1 | network_drop_mid_c5 | sigkill_mid_cycle | retention_expiry | slot_invalidation | Verdict | s | Run |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | ---: | --- |
| PostgreSQL | 14 | pass | pass | pass | pass | pass | pass | passed | 275 | `rc-l2p` |
| MySQL | 8.0 | pass | pass | pass | pass | pass | n/a | passed | 187 | `rc-l2m-r` |
| Oracle | 19c | pass | pass | pass | pass | pass | n/a | passed | 933 | `rc-l2o-now` |

MySQL 8.0 first attempt (`rc-l2m`, 224 s, host load average about 60):
`sigkill_mid_cycle` failed only on its C1 path (the resume after the
`c1_apply_commit` failpoint exited with `connect_timeout`; snapshot and C5
paths passed, positions monotonic) and `retention_expiry` failed with
`connect_timeout` before any fault. Both are the 10 s connection timeout under
load; the rerun at load average about 8 passed every case.

### Defect replay gate

| Run | SHA | Caught | Error | Status |
| --- | --- | ---: | ---: | --- |
| `rc-gate` | `f6c8f8b2a` | 12 | 2 (corpus drift, section 2) | failed |
| `rc-gate2` | `edc0cd171` | 9 | 5 | failed: the five L1 detectors were refused by the default two-container preflight while the lanes ran (`non_oracle_slots_occupied`); harness, all five were caught in `rc-gate` |
| **`rc-gate3`** | **`edc0cd171`** | **14** | **0** | **passed**, complete corpus, cleanup ok, 434 s |

## 4. Quickstart and ERP

Lite image rebuilt from this tree (`IMAGE=open-ontology/lite:v03-arm64 docker
buildx bake ...`, 1,478 s under full suite load); npm package staged with
`packages/cli/scripts/stage.mjs` from `target/debug/oo`, `oo-mysql-doctor`,
`mcp-gateway` and the image digest (`evidence/rc-qs-stage`).

| Run | QS-01 | QS-02 | QS-03 | QS-04 | QS-05 | Install to first answer (warm) | Note |
| --- | --- | --- | --- | --- | --- | ---: | --- |
| `rc-qs` (10:43) | pass | pass | pass | pass | not passed | 14.9 s | `down` exit 0; the before/after Docker inventory differed by 2 containers and 1 volume, all lane-ta Oracle containers started by the concurrent L1 lanes (checked by hashing the owned IDs); harness interference |
| **`rc-qs2`** (11:17) | **pass** | **pass** | **pass** | **pass** | **pass** | 15.6 s | inventories equal |

PostgreSQL 17.10 serving; 120 source orders, 60 visible to the analyst
(fixture RLS); the denied example's `COLUMN_NOT_ALLOWED` audit row was read
back from the control DB; the Claude, Cursor and Codex MCP configurations
each completed readiness, query and answerability.

ERP: `python3 -m unittest discover -s benchmarks/ab -p 'test_*.py'`: 39 passed
(exact-decimal scoring, frozen-gold binding, logistics bank). The live ERP gold
preflight has no model-free entry point (it runs inside `run.py ab` after an
installed MySQL ERP publication); not run.

## 5. T10 remaining D24 items (S6-H-02)

Loader: `tools/oo-load` release with `mysql-binlog` at `54a02d374`'s source
(SHA-256 `914d9a0d...`, product SHA recorded as `f6c8f8b2a` plus the handoff
fix). Source 4 CPUs / 8 GiB, 2,000,000 rows, target on tmpfs, fsync canary on,
`--quiet-cpu-percent 25`, rate auto (half the settled rate). Phases shortened
to `--minutes 5` (PostgreSQL) and `--minutes 4` (MySQL); each C5 sub-phase
repeats whole cycles for half that, so same-length baseline slices exist
(the harness computes them only for phases shorter than the baseline).

| Engine | Phase | p50 / p95 / p99 ms | Req/s | Err | Seconds (cycles) | Δp95 / Δp99 / Δrate % | Canary writes >100 ms | Without canary windows (Δp95 / Δp99) | D24 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| PostgreSQL 17 | baseline | 0.404 / 0.721 / 1.305 | 3729.2 | 0 | 300.0 | 0.0 / 0.0 / 0.00 | 0 (max 8 ms) | - | pass |
| PostgreSQL 17 | c5_changed | 0.375 / 0.685 / 1.347 | 3729.2 | 0 | 179.9 (5) | -5.0 / 3.2 / -0.00 | 0 (max 46 ms) | pass (-5.0 / 3.2) | **pass** |
| PostgreSQL 17 | c5_full_changed | 0.411 / 1.777 / 12.685 | 3729.1 | 0 | 209.7 (2) | 146.5 / 872.0 / -0.00 | 0 (max 47 ms) | fail (146.5 / 872.0) | **fail** |
| MySQL 8.4 | baseline | 0.546 / 5.356 / 3978.172 | 2035.0 | 0 | 240.0 | 0.0 / 0.0 / 0.00 | 4 (max 5,602 ms) | - | pass |
| MySQL 8.4 | initial | 0.526 / 113.483 / 672.264 | 2034.9 | 0 | 25.6 | 2019.0 / -83.1 / -0.00 | 3 (max 514 ms) | pass (-4.1 / -22.3) | fail |
| MySQL 8.4 | initial_duty100 | 0.510 / 3.110 / 3.998 | 2035.0 | 0 | 12.5 | -41.9 / -99.9 / -0.00 | 0 | pass (-1.6 / -15.0) | pass |
| MySQL 8.4 | c1 | 0.548 / 44.691 / 4411.334 | 2035.0 | 0 | 240.0 | 734.5 / 10.9 / 0.00 | 6 (max 6,004 ms) | fail (0.3 / 191.1) | fail |
| MySQL 8.4 | c5_idle | 0.573 / 3.446 / 7.116 | 2035.0 | 0 | 132.4 (6) | -35.7 / -99.8 / -0.00 | 0 (max 47 ms) | fail (9.0 / 51.2) | pass |
| MySQL 8.4 | c5_changed | 0.614 / 3.691 / 10.286 | 2035.0 | 0 | 134.3 (5) | -31.1 / -99.7 / -0.00 | 2 (max 387 ms) | pass (13.0 / 9.9) | pass |
| MySQL 8.4 | c5_full_idle | 0.634 / 23.254 / 222.103 | 2035.0 | 0 | 147.0 (3) | 334.2 / -94.4 / -0.00 | 1 (max 191 ms) | fail (188.8 / 4,301.8) | fail |
| MySQL 8.4 | c5_full_changed | 0.676 / 372.353 / 4619.087 | 2035.0 | 0 | 174.0 (3) | 6852.7 / 16.1 / -0.00 | 2 (max 5,714 ms) | fail (17.0 / 1,258.7) | fail |

No product call failed in either run; no foreground errors; throughput
unchanged in every phase. Runs `t10-rc-postgres-17` (1,052 s, summary
`result: fail`) and `t10-rc-mysql-84` (1,559 s, `result: fail`).

- **PostgreSQL 17 changed C5 (lite period 24): pass**, p95 -5.0%, p99 +3.2%,
  five cycles; E473 run B had failed it at p99 +83.7%. Baseline 50 s
  slices failed p99 2 of 26; same-length slices 0 of 14.
- **PostgreSQL 17 full-read changed C5 (period 1): fail**, p95 +146.5%, p99
  +872%. The canary saw no host IO stall; the excess sits in five consecutive
  10 s windows late in the phase (window p95 2.4-52 ms against about 0.65 ms),
  the hot-row convoy pattern of E473; same-length baseline slices failed 0 of
  11, so the fixture alone does not produce it. Still the open residual of
  E473 (period 1 is not the lite default; the default reads in full only every
  24th cycle).
- **MySQL 8.4: fixture-limited again.** The baseline itself had four canary
  writes over 100 ms (up to 5.6 s) and p99 3,978 ms, so relative deltas
  against it are not meaningful (most p99 deltas are strongly negative). Using
  the canary-adjusted view: initial, 100% duty and changed C5 pass; C1 passes
  p95 (+0.3%) but fails p99 (+191%); idle C5 is at p99 +51.2% (just over);
  full-read C5 fails. MySQL remains not signed off; repeat on storage that
  sustains the fixture's fsync rate (E473 guidance unchanged).
- Oracle 23 defaults (E472) were not remeasured, as requested.
- The PostgreSQL run predates the `--c5-variants` result fix, so its
  `summary.json` lists the two unmeasured idle variants as missing; per-phase
  verdicts are unaffected.

## 6. Wall clock

| Stage | Start-end (PST) | Seconds |
| --- | --- | ---: |
| Build: oo-cli/oo-builder/oo-gateway, oo-load debug + release + failpoints (`mysql-binlog`), mysql-doctor, release-coordinator; compile-error triage and fix | 09:08-09:16 | about 480 |
| T10 driver venv (psycopg/pymysql missing on the host Python) | 09:16-09:18 | about 120 |
| T10 PostgreSQL 17 | 09:18:14-09:35:46 | 1,052 |
| Idle: my chained start waited on a `pgrep` pattern that matched its own wait loops | 09:35:46-09:41:18 | 332 |
| T10 MySQL 8.4 | 09:41:18-10:07:17 | 1,559 |
| Fast suite start (L0 + seven L1 lanes + image build) | 10:07:23 | |
| L0 first / rerun | 10:08-10:13 / 10:41-10:43 | 316 / 82 |
| L1 PostgreSQL lanes (3) | 10:08-10:33 | 1,475 / 1,521 / 1,519 |
| L1 MySQL lanes (2) | 10:08-10:30 | 1,366 / 1,216 |
| L1 Oracle 21c+23ai lane / 11g lane | 10:08-10:36 / 10:07-10:49 | 1,674 / 2,530 |
| Defect gate first / interfered / clean | 10:13-10:24 / 10:25-10:32 / 10:34-10:41 | 660 / 442 / 434 |
| Lite image build | 10:08-10:32 | 1,478 |
| L2 PostgreSQL 14 / MySQL 8.0 first / rerun | 10:32-10:37 / 10:30-10:34 / 10:36-10:39 | 275 / 224 / 187 |
| L1 Oracle 23ai rerun / Oracle 19c lane | 10:46-10:56 / 10:38-11:20 | 557 / 2,544 |
| L2 Oracle 19c | 10:50-11:06 | 933 |
| QS first / rerun | 10:43 / 11:17 | 26 / 30 |
| Fast stage (L0 through the last Oracle cell, with reruns) | 10:07:23-11:20:48 | 4,405 |
| **Total** | **09:08:03-11:20:48** | **7,965 (2 h 13 min)** |

Per expected time (TA-01 single-lane): PostgreSQL and MySQL cells took
1.5-2x their TA-01 time under seven-lane load (for example PostgreSQL 13 949 s
vs 225 + 69 + 132 s); Oracle 11g took 2,530 s (TA-01: 27 + 80 + 368 s plus
the then-cached snapshot): its fixture snapshot was cold after the host
cleanup and its thick worker and T1 run under amd64 emulation. Oracle 19c
starts cold in about 500 s per container under load. The fast stage took 73 minutes
including every rerun (target 90); PostgreSQL and MySQL L1 finished in
26 minutes and their L2 by 10:39. The Oracle 19c lane (cold 19c Enterprise
starts, about 500 s each, in the shared cell, T2 and T4) set the end. The
total of 2 h 13 min exceeds the 2-hour budget by 13 minutes; T10 took 49
minutes against its 40 (5.5 minutes of it the idle gap above).

## 7. Commits

- `54a02d374` Fix the oo-load mysql-binlog build and RC fast-suite harness drift (`Plan: S6-H-01`)
- `edc0cd171` T10: remeasure selected C5 sub-phases with --c5-variants (`Plan: S6-H-02`)
- this report and ledger row (`Plan: S6-H-01`)
