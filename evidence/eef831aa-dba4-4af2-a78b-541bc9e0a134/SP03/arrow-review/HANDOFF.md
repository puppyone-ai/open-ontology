# Handoff

Verdict: REQUEST_CHANGES for the R2-S slice on `75b993d0b578fa863a32c64abc0073ed92d51a5d`.

Do not sign R2-S. Do not mark R1, R2, or the reader accepted. Do not approve the bounded TLS integration. The a133 `init_mut` tail remains the driver gate, on a tree this review did not build.

## Defect to send back

`REVOKE SELECT` after `TableEof` of the previous table and before the next table returns `Privilege` / `insufficient_privilege` from the scan, not `Privilege` / `select_not_granted` from recheck.

Lines: `sql.rs` 387-394, `snapshot.rs` 1424-1426 and 1967-1984, `stream.rs` 509-520, `error.rs` 193.

Reproducer: `overlay/pg_stream_review.rs` `acl_guard`, log lines 2708-2720 in `linux/functional/postgresql.log`. No `acl_b` row was emitted. `finish` was not `SnapshotFinished`.

## Held, with the numbers in REPORT.md

One RR owner across parent, child, empty, and zerocol. Arrow values equal the pre-writer oracle, including NULL and the zero-column presence batch. Writer rows stayed out of that snapshot. Demand bounds, one batch-byte split (`aa`, `bbbb`), and one oversize `max_bytes` refusal. Schema `ALTER` is `schema_drift` before the drifted scan. Numeric PK streams with `key` false and `read_probe` returns `probe_key`. Portal fetches wait for demand. `ROLLBACK TO` then `RELEASE` follows the portal on the success path. Finish, abort, and drop release the backend. Generation reuse is rejected. Reader SQL has zero DML, DDL, and COMMIT.

Original `pg_stream_live` and `pg_stream_bounds` exited 0 on this composition. Linux 1.94 lib exited 0 (57 passed). Scoped clippy exited 0.

## Composition

Archive of `75b993d0`, tree `a1250d01ae3c8ac2e8ee81ce29ae9ecfafe71966`, vendor blob `ba2e7dc31b1e8bd133c3a4827f55255312dde522` (includes `79515fcf`). Author evidence `b2bb0215` was built on `ccd93e1` vendor blob `f41e0e4d`. Those are different builds. The a133 worktree blob `3ba2b875` was rejected by the smoke check.

## Evidence

`linux/smoke/` exits and console. `linux/functional/` decisive exits, outcomes, console, and `postgresql.log`. `linux/functional/attempt1/` and `attempt2/` are overlay compile failures before the decisive run. `pg/` has the fixture and a password-free pointer. `overlay/pg_stream_review.rs` is the independent test.

Images: rust `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, postgres `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, both arm64.

## Cleanup

Removed only `oo-a134-msrv-smoke` `02b497d0eeaea9c019d5bbd99245232dd7f50959c510dda1401dd4e107e0660d`, `oo-a134-msrv-review` `0a5c174ba5fb34c45533a717c708f0fb8bea87b93a03055a9fd81ce3d140daaa`, `oo-a134-pg` `9a9fbd9c2f860aa4359b401351508b1cc7c363e3041e035f6cdc1d99f3b8178e`, volumes `oo-a134-eef831aa-cargo`, `oo-a134-eef831aa-target`, `oo-a134-eef831aa-pglog`, `oo-a134-eef831aa-pgdata`, network `oo-a134-eef831aa-net`, and `/tmp/oo-a134-eef831aa` plus `/tmp/oo-a134-eef831aa-dba4-4af2-a78b-541bc9e0a134`. Recorded in `cleanup.txt` at `2026-10-03T16:18:20Z`. No prune and no other task's containers.
