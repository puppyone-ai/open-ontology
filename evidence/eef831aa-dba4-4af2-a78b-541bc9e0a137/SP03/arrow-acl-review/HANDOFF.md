# Handoff

Verdict: APPROVE LIMITED for the R2-S slice on `dd5bf44160f949c0ac21a0ce3ddf5ee5e90b4200`.

Do not mark full R1, full R2, or the reader accepted. Do not mark R3 or R4 accepted. a135 remains the limited acceptance of the `bd77b252` receive tail. This run does not widen that acceptance.

## What closed the a134 defect

A committed `REVOKE SELECT` after `TableEof` of `acl.acl_a` and before `acl.acl_b` returns `Privilege` / `select_not_granted` from the current-catalog guard and from `finish`. The PostgreSQL window is `linux/functional/acl-window.txt` lines 111-126. No `acl_b` scan, no `42501`, no second `SAVEPOINT oo_stream_lock`. Pids 145 and 146 were released.

`42501` is still `insufficient_privilege` (`error.rs` 193). `stream.rs` blob `e7b5a6cc` is the `75b993d0` blob. The guard still runs before the savepoint.

## Runs

Smoke on the exact archive: lib 57 passed, scoped clippy 0, docker-run 0. `linux/smoke/`.

Original `pg_stream_acl_live`, `pg_stream_live`, and `pg_stream_bounds`: exit 0 once. Live Arrow payload 59 against the pre-writer oracle. Bounds kept the demand-gated execute. Numbers are in `REPORT.md`.

Inherited grant `colg_parent` on `colg.via_parent`: exit 0. Arrow int32 3, `SnapshotFinished`, both backends released. The grantee in `relacl` is `colg_parent`, not `oo_a131_reader`. `linux/column-grant/`.

The first overlay open of a column-only grant exited 101 before a batch. `check_relation` still requires table `SELECT`. The expression case itself returns true for that grant (`pg/expression-probe.txt`). PostgreSQL 17.10 clears `attacl` on `REVOKE SELECT ON TABLE`, so that shape is not a post-open survivor. Detail is FINDINGS section 4.

## Composition

Archive tree `dd20253a1e7bab45bb5a7d34677ad0b3154644bc`. `buffered.rs` blob `3ba2b875` matches the a135 driver. `error.rs` and `types/mod.rs` match `75b993d0`. Author blob `87c4b403` is not in this object store. Do not treat author evidence `173e6eb0` as a hash-verified inventory.

## Evidence

`linux/smoke/` exits and console. `linux/functional/` original-test exits, outcomes, console, `postgresql.log`, and `log-audit.txt`. `linux/functional/attempt1/` is the column-only open failure. `linux/column-grant/` is the inherited-grant run, and `attempt-column-revoke/` keeps the earlier column-revoke failure. `pg/` has the fixture, the expression probe, and a password-free pointer. `overlay/pg_stream_acl_review.rs` is the independent test.

Images: rust `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, postgres `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, both arm64.

## Cleanup

Own containers, volumes, network, and `/tmp` paths for this session are listed in `cleanup.txt`. No prune and no other task's containers. Local evidence commit only. No push.
