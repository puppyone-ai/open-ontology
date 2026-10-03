# Handoff

Source commit `dd5bf44160f949c0ac21a0ce3ddf5ee5e90b4200` on parent `f946bd90c8c0ca72343bce94f6aeca865db968ed`. Fixed input `bd77b2520bcc3d63517bc49309002f25b58544b1`.

The current-catalog guard now returns `Privilege` / `select_not_granted` from the READ COMMITTED row's `relacl` and `attacl` before `SAVEPOINT oo_stream_lock` and before the scan. The decisive PostgreSQL log and outcomes are in `linux/functional/`. Numbers are in `REPORT.md`. The mechanism is in `FINDINGS.md`.

Original R2-S fixed review is still required. Do not sign R2-S. Do not mark the whole reader accepted.

`stream.rs`, `pg_stream_live.rs`, and `error.rs` are unchanged. `42501` remains `insufficient_privilege`. The new test is one revoke of table `SELECT` on the second table after the first `TableEof`. It is not an ACL, type, or security matrix.

Cleanup of the a136 containers, volumes, network, and `/tmp` paths is in `cleanup.txt`. No prune and no other task's containers.
