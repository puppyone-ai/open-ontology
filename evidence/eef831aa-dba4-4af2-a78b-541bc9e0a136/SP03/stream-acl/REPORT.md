# Report — a136 current ACL pre-scan

Source commit `dd5bf44160f949c0ac21a0ce3ddf5ee5e90b4200`, parent `f946bd90c8c0ca72343bce94f6aeca865db968ed`. Fixed input `bd77b2520bcc3d63517bc49309002f25b58544b1`. Linux arm64 Rust 1.94.0 (`4a4ef493e`, 2026-03-02). PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1).

Images: rust `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, postgres `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.

The functional builder archived `bd77b252` and copied only `snapshot.rs`, `sql.rs`, `tests/pg_stream_acl_live.rs`, and `contracts/source/pgsql/bounded-stream-v1.md`. `stream.rs` blob `e7b5a6cc1714c67e818899788a09e0315b93b3ed` and `pg_stream_live.rs` blob `95e9d2b0a9151d4aef2c274d5b234b59da8838f6` match that commit. Those two files are not in `dd5bf441`.

## Smoke, before the edit

`linux/smoke/`: docker-run 0, lib 0 (57 passed), clippy 0, snapshot 0, toolchain 0. Elapsed 77 seconds. Privileged false, cap-drop ALL, network `oo-a136-eef831aa-net`, memory 8589934592, OOMKilled false.

## Functional, decisive run

`linux/functional/` on the private network. Host config: ExitCode 0, Privileged false, CapDrop ALL, Memory 8589934592, OOMKilled false. Elapsed 16 seconds.

| phase | exit |
| --- | --- |
| clippy-component | 0 |
| clippy `-p oo-connectors --all-targets -D warnings` | 0 |
| lib | 0 (57 passed) |
| pg_stream_acl_live | 0 |
| pg_stream_live | 0 |
| pg_stream_bounds | 0 |
| docker-run | 0 |

Vendor `sqlx-postgres` printed two existing `unfulfilled_lint_expectations` warnings. Clippy still exited 0.

ACL outcome:

`acl_a_id=7 acl_a_eof=1 generation=1 data_pid=474 data_start=2026-10-03 17:10:02.738085+00 guard=select_not_granted finish=select_not_granted reader_after=6 reader_writes=0 second_savepoint=0 second_scan=0 backends_gone=2`

Live outcome kept the pre-writer oracle: parent 4, child 3, empty 0, zero 2, payload 59. Arrow preimage 132 bytes. `admin_sees_late_parent=1`. Reader audit `dml_ddl_commit=0`, writer writes 2. Bounds refused `max_value` and `max_row`, and the slow consumer added one execute only on the next demand.

`postgresql.log` has no reader or superuser password. In the ACL window, before the admin `REVOKE`: one `SAVEPOINT oo_stream_lock` and two `SELECT id FROM acl.acl_a` lines (portal and fetch). After the revoke: one current-catalog guard statement. `savepoint_after=0`, `SELECT id FROM acl.acl_b` count 0, `permission denied` count 0.

Overlay blobs recorded at that launch: sql `ede6e5d30796fa8ec79696e47df80211438670bf`, contract `f8842d8193bc71b7a112c9f32c14e2af8574cbc7`, test `05c5c331b232cce11bd2d8dddfc9640532f8a145`, snapshot `87c4b403d332cfcb279e975dadc69a81360b3b33`. The committed snapshot blob is `3991674d164ff4fcf5150dded2105a1144028f53`. The difference is the `ObservedAttr.select` doc comment, written after the green run.

## Scope

One grant/revoke case. `stream.rs` and `error.rs` are unchanged, so `42501` remains `insufficient_privilege`. Original R2-S fixed review is still required. This run does not sign R2-S and does not accept the whole reader.
