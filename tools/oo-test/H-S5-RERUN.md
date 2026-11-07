# H-S5 consolidated rerun, 2026-11-05

Status: partial. S5-H-01 and S5-H-02 remain open.

Rebase onto `v0.2/product-loops` reported up to date. The previous blocked
attempt and its 140 rows are preserved. This run appends 425 raw-hash-verified
rows (565 total) and regenerates the matrix without editing its rules.

Validation/run receipt: `h-s5-rerun-20261105`.
Consolidated merge: `h-merged-2a17249a-9d09-45dc-bdab-b7752397bddd` (exit 1, explicit failures retained).

## Commands and scope

- Built prerequisites first: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_TARGET_DIR="$PWD/target" cargo build --manifest-path tools/oo-load/Cargo.toml --locked --features mysql-binlog` (pass).
- Initial `OO_TEST_LANE=h tools/oo-test/oo-test matrix --all --suites T0-T4,T7,T8` visited all 14 targets; missing product hooks and Oracle 19c image resolution failed explicitly.
- For each available version, sequentially ran `oo-test matrix --engine ENGINE --versions VERSION --suites T0-T4,T7,T8 --hook T1=tools/oo-test/t1.py`, then `t2.py` (PG) or `t2_p0.py` (MySQL/Oracle), then `oo-test t3t4 --disk-wait-minutes 30`. No T5/T6/model calls.
- Full snapshot fixtures kept the default 2,000,000 events and 200 small tables. T2/T3/T4 retain their documented compact fixtures; no scale equivalence is claimed.
- PostgreSQL 13-16 snapshot retries: `oo-test matrix --engine postgres --versions 13,14,15,16 --suites T0,T1,T8 --hook T1=tools/oo-test/t1.py` (exit 0, but T0 rows remain fixture-only skips).
- Oracle 23 snapshot retry used the same suite selection (exit 1).
- `oo-test matrix --merge-only --merge-evidence ...` validated and merged every run. The dispatch and retry JSON receipts contain exact commands and exits.
- `python3 specs/scripts/gen_support_matrix.py` and `--check --self-test`: pass, 565 rows and 7 derivation cases.

## Real suite verdicts

| Engine | Measured versions | T1 / snapshot T8 | T2 C1 / C5 | T3 C1 / C5 | T4 |
|---|---|---|---|---|---|
| MySQL | 5.7.44, 8.0.44, 8.4.7, 9.5.0 | pass / pass | pass / pass | pass / pass | pass |
| PostgreSQL | 13.23, 14.24, 15.19, 16.15, 17.11, 18.6 | pass / pass after retained retries | pass / pass | pass / pass | pass |
| Oracle | 23.5.0.24.07 | fail / partial | pass / pass | fail / fail (retention not run) | pass |
| Oracle | 21.3.0.0.0, 11.2.0.2.0 | fail / partial | C1 not implemented (fail rows) / pass | skip / pass | C1 skip, C5 pass |
| Oracle | 19c (unmeasured) | blocked E014 | blocked | blocked | blocked |

Incremental T8 passes for the successful incremental variants. A standalone
Oracle XE T2 command exiting 0 only accepts its implemented C5 path; its C1
rows are non-passes. T0 is not product doctor: all executed fixture T0 rows
are skips. No version has passing T7. MySQL 5.7 and Oracle XE 11g/21c ran
as linux/amd64 under emulation; other observed versions ran natively.

## Generated version x capability levels

These are exactly the generator's output, not hand-selected acceptance.
Oracle image-version identifiers from blocked runs remain separate from
measured database versions. `schema_change` for XE covers C5 only; C1 is skipped.
T5/T6 capabilities are absent.

| Target | Version | full | cdc | digest | schema_change |
|---|---|---|---|---|---|
| mysql57-selfmanaged | 5.7.44 | experimental | experimental | verified | verified |
| mysql8-selfmanaged | 8.0.44 | experimental | experimental | verified | verified |
| mysql8-selfmanaged | 8.4.7 | experimental | experimental | verified | verified |
| mysql9-selfmanaged | 9.5.0 | experimental | experimental | verified | verified |
| oracle-11g-xe | 11.2.0.2 | designed | designed | designed | designed |
| oracle-11g-xe | 11.2.0.2.0 | experimental | designed | verified | verified |
| oracle-19c-selfmanaged | 19c | designed | designed | designed | designed |
| oracle-21c-xe | 21.3.0 | designed | designed | designed | designed |
| oracle-21c-xe | 21.3.0.0.0 | experimental | designed | verified | verified |
| oracle-23ai-selfmanaged | 23.5 | designed | designed | designed | designed |
| oracle-23ai-selfmanaged | 23.5.0.24.07 | designed | experimental | experimental | verified |
| postgres-selfmanaged | 13.23 | experimental | experimental | verified | verified |
| postgres-selfmanaged | 14.24 | experimental | experimental | verified | verified |
| postgres-selfmanaged | 15.19 | experimental | experimental | verified | verified |
| postgres-selfmanaged | 16.15 | experimental | experimental | verified | verified |
| postgres-selfmanaged | 17.11 | experimental | experimental | verified | verified |
| postgres-selfmanaged | 18.6 | experimental | experimental | verified | verified |

All earlier generated cells were designed. No previously verified cell was
silently retained or promoted from a process exit. Full/CDC remain below
verified; Oracle 23 digest remains experimental. Its full level is designed
because the latest snapshot retry failed T1/T8. XE full is experimental only
because incremental T8 passes, not because its snapshot is correct.

## Failures, repairs, and remaining gaps

1. Initial all-target orchestration still requires product hooks; it does not
   implement doctor or audited incremental/fault workloads. Missing hooks are
   explicit failures, not inferred acceptance. Standalone rows are merged by
   hash; independent fixtures are not one T7 audit session.
2. PostgreSQL 13 first failed `session_identity`: role-level audit GUCs conflict
   with the connector's startup guard. The fixture now enables global logging
   and filters CSV records by reader identity. No source policy was relaxed.
3. PostgreSQL 14-16 failed corrected `snapshot_connect`; preserved PG16
   collector logs show SIGTERM backend exits at exec relay teardown and source
   recovery. Hooked matrices now reuse standalone loopback transport. PG17/18
   and subsequent PG13-16 full-size retries passed. Failed attempts remain.
4. Oracle 23 initially raised ValueError parsing PostgreSQL bytea JSON. Oracle
   comparison now accepts its hex prefix and decodes JSON decimal fractions
   without float rounding; regression tests preserve exact equality rules.
5. All three Oracle versions still fail TYPE_EDGES equality. Offline replay of
   the saved comparison outputs isolates the same timestamp difference: equal
   UTC instant, source offset +08:00 versus target offset +00:00. Separate
   `ORIGINAL_ZONE` remains +08:00. The comparator's offset-sensitive contract
   was not weakened; coordinator/engine owner must resolve that representation
   contract before acceptance. Raw per-version difference receipts are in the
   validation run directory.
6. Oracle 23 T3 archive retention expiry was not injected: the pinned slim
   image has no RMAN. Other applicable fault cases passed, including resetlogs,
   but the entire T3 row is fail. No missing test is counted as passed.
7. T7: MySQL collected only four boundary statements (no workload coverage,
   fixture restart resets logging); PG17/18 collected 48,714 statements each
   with 45,487 classifier rejections; Oracle 23 collected 199,682 statements
   with 81,952 rejections. All lack audited T2 dependencies. Earlier PG audit
   collection failed during recovery; XE audit collection failed SQL*Plus.
   Classifier rejections are not proof of source writes. None is a T7 pass.
8. Oracle 19c remains E014 (licensed image unavailable); XE C1 is not established
   by these runners. Three consecutive stable nights and owner delivery of
   notifications are not proved by this single daytime run.

No shared-file patch is requested and no Cargo.lock changed. The only granted
shared artifacts changed are the appended evidence index and generated matrix.
No engine, fixture gold, migration, scheduler, token, customer system, other
worktree, or unrelated Docker resource was changed.

## Local checks and cleanup

- `cargo check --workspace --locked`: pass (debug info/incremental disabled to conserve disk).
- `python3 -m unittest discover -s tools/oo-test -p 'test_*.py'`: 85 passed.
- `python3 -m unittest discover -s ci -p 'test_*.py'`: 39 passed.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.
- No Rust edits; touched-crate formatting/unit tests are not applicable.
- Disk was checked before targets with a 20 GiB floor and a maximum 30-minute
  wait. No rerun target was disk-blocked. Cached pinned images were reused;
  uncached pulls require 25 GiB. At most two database containers ran at once.
- Per-target owned-resource checks and final lane-H container/volume listings
  are empty. No images were removed; no unrelated resources were stopped.

## Raw run catalog

All paths below are git-ignored evidence. Matrix exit 1 is expected when any
requested suite remains absent or failed; exit 0 alone never supplies a row.

| Invocation | Exit | Evidence directory |
|---|---|---|
| initial-matrix | 1 | `evidence/h-s5-20261104T223729116961Z-919f8baf-df26-4197-ac8c-db3fcd1b2a00` |
| mysql-5_7-matrix | 1 | `evidence/h-s5-20261104T223729212745Z-d5bb20b6-a5f5-4b49-8ec4-201938ce6c67` |
| mysql-5_7-t2 | 0 | `evidence/h-s5-rerun-mysql-5_7-t2` |
| mysql-5_7-t3t4 | 0 | `evidence/h-s5-rerun-mysql-5_7-t3t4` |
| mysql-8_0-matrix | 1 | `evidence/h-s5-20261104T230052025088Z-67faaa18-3870-42fe-a7a9-74cc764c1d8a` |
| mysql-8_0-t2 | 0 | `evidence/h-s5-rerun-mysql-8_0-t2` |
| mysql-8_0-t3t4 | 0 | `evidence/h-s5-rerun-mysql-8_0-t3t4` |
| mysql-8_4-matrix | 1 | `evidence/h-s5-20261104T232109462283Z-935f0e17-a4c2-45bd-b1a0-bed0a7f12de7` |
| mysql-8_4-t2 | 0 | `evidence/h-s5-rerun-mysql-8_4-t2` |
| mysql-8_4-t3t4 | 0 | `evidence/h-s5-rerun-mysql-8_4-t3t4` |
| mysql-9-matrix | 1 | `evidence/h-s5-20261104T234914386754Z-4f498714-e2a1-4e4c-ab9b-12b29fada03a` |
| mysql-9-t2 | 0 | `evidence/h-s5-rerun-mysql-9-t2` |
| mysql-9-t3t4 | 0 | `evidence/h-s5-rerun-mysql-9-t3t4` |
| postgres-13-matrix | 1 | `evidence/h-s5-20261105T001152227532Z-8dae6d98-0e9d-4755-8f55-a8d66964f32d` |
| postgres-13-t2 | 0 | `evidence/h-s5-rerun-postgres-13-t2` |
| postgres-13-t3t4 | 0 | `evidence/h-s5-rerun-postgres-13-t3t4` |
| postgres-14-matrix | 1 | `evidence/h-s5-20261105T004213261923Z-be22158f-1548-4219-aaec-a6bfc4664dd5` |
| postgres-14-t2 | 0 | `evidence/h-s5-rerun-postgres-14-t2` |
| postgres-14-t3t4 | 0 | `evidence/h-s5-rerun-postgres-14-t3t4` |
| postgres-15-matrix | 1 | `evidence/h-s5-20261105T011455864866Z-f8042d1e-652e-4917-af6d-f54da83adb4b` |
| postgres-15-t2 | 0 | `evidence/h-s5-rerun-postgres-15-t2` |
| postgres-15-t3t4 | 0 | `evidence/h-s5-rerun-postgres-15-t3t4` |
| postgres-16-matrix | 1 | `evidence/h-s5-20261105T015327932652Z-9ceeff10-8369-47e3-9753-c349658cd37b` |
| postgres-16-t2 | 0 | `evidence/h-s5-rerun-postgres-16-t2` |
| postgres-16-t3t4 | 0 | `evidence/h-s5-rerun-postgres-16-t3t4` |
| postgres-17-matrix | 1 | `evidence/h-s5-20261105T022632727199Z-d0108f97-3c57-40a8-abe3-80f0fabc9b06` |
| postgres-17-t2 | 0 | `evidence/h-s5-rerun-postgres-17-t2` |
| postgres-17-t3t4 | 0 | `evidence/h-s5-rerun-postgres-17-t3t4` |
| postgres-18-matrix | 1 | `evidence/h-s5-20261105T030408698456Z-f8025bfa-cad9-4d12-9894-eed3a6da13e6` |
| postgres-18-t2 | 0 | `evidence/h-s5-rerun-postgres-18-t2` |
| postgres-18-t3t4 | 0 | `evidence/h-s5-rerun-postgres-18-t3t4` |
| oracle-23-matrix | 1 | `evidence/h-s5-20261105T034925700332Z-1e01f194-e21d-41a7-8b17-220aef31fba2` |
| oracle-23-t2 | 0 | `evidence/h-s5-rerun-oracle-23-t2` |
| oracle-23-t3t4 | 1 | `evidence/h-s5-rerun-oracle-23-t3t4` |
| oracle-21-matrix | 1 | `evidence/h-s5-20261105T040814609527Z-0893b66d-891c-45c3-a684-5cfb98353a9c` |
| oracle-21-t2 | 0 | `evidence/h-s5-rerun-oracle-21-t2` |
| oracle-21-t3t4 | 0 | `evidence/h-s5-rerun-oracle-21-t3t4` |
| oracle-11-matrix | 1 | `evidence/h-s5-20261105T044627131328Z-fac2ad67-bed5-43c8-9cef-7f8c71e8cad8` |
| oracle-11-t2 | 0 | `evidence/h-s5-rerun-oracle-11-t2` |
| oracle-11-t3t4 | 0 | `evidence/h-s5-rerun-oracle-11-t3t4` |
| merged | 1 | `evidence/h-merged-607bbc63-588e-4938-83e2-4e663c95868b` |
| postgres-snapshot-retry | 0 | `evidence/h-s5-20261105T050700882953Z-27f31925-93f6-41bc-a376-2ac0086dfb33` |
| oracle-snapshot-retry | 1 | `evidence/h-s5-20261105T060418347098Z-0500bb9a-5db3-4de2-9f63-405948707e02` |
| consolidated merge | 1 | `evidence/h-merged-2a17249a-9d09-45dc-bdab-b7752397bddd` |
