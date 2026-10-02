# Event project binding

`jobs-event-project/1` is an expand migration after `jobs-control/1`. It makes `jobs.task_events.project_id` equal the task's `project_id`. A068-F2 is that gap: a real `LOGIN` member of `oo_jobs_runtime` inserted project P2 on a task whose project was P1. The `jobs-control/1` approval for SP02-W01-a stands. This file is inside the task commit and does not name that commit. The parent is `parent.txt`.

Only this event-binding subitem is complete. Full SP02, the jobs driver, a 100-worker run, backup and restore, and the vertical flow stay open.

## Database rule

`0002_event_project_binding_expand.sql` runs as `oo_jobs_owner` inside the applicator transaction that already holds `pg_advisory_xact_lock(6401202601)`. It locks `jobs.task_events` in `SHARE ROW EXCLUSIVE`, then `jobs.tasks`. That is the same order as an event insert and as `ADD FOREIGN KEY`.

If any event row has no task with the same `id` and `project_id`, the migration raises `OO004` and the transaction rolls back. The message states the row count and that this migration does not rewrite, delete, or forge a correction. Repair of that history is a later audited plan. This task does not invent one. Matching historical rows are not updated.

When the count is zero, the migration adds non-deferrable `UNIQUE (id, project_id)` on `jobs.tasks`, then non-deferrable `FOREIGN KEY (task_id, project_id) REFERENCES jobs.tasks (id, project_id) ON DELETE RESTRICT ON UPDATE RESTRICT`. `id` stays the primary key. The unique constraint exists first because the foreign key needs that column order.

Existing append-only and audit triggers stay. The only new grant is `SELECT` on `jobs.event_project_readiness` to `oo_jobs_runtime`. The view is `security_invoker`. `jobs.service_readiness` still reports only `jobs-control/1`. There is no new function and no `SECURITY DEFINER`.

A direct insert may still store a caller-chosen note, generation, lease owner, or state pair. The same-project probe committed generation `99` and lease owner `spoofed-worker` and left the task generation at `0` with an empty lease owner (`0||99|spoofed-worker|true`). Equality is the acceptance scope. Event contents stay internal and worker-provided. This is not a tamper-proof audit, and it does not authenticate a worker. Row security stays off.

Ready for this version means `jobs.service_readiness` matches the `jobs-control/1` file, the bootstrap checksum matches `0000`, and `jobs.event_project_readiness` matches `0002`. A zero digest makes the new ready flag false while `jobs-control/1` readiness stays true. Fresh install still stores the caller-supplied checksum. Comparing that stored digest to the file remains an external check. Bootstrap checksum trust stays open.

## Pins

`shasum -a 256` and Python `hashlib.sha256` agree on the workspace files and on the Linux snapshot. `tests/event_project_binding_contract.rs` hashes the same bytes with `/usr/bin/sha256sum` or `/usr/bin/shasum`, not `include_str!`.

| Version | File | SHA-256 |
|---|---|---|
| `jobs-bootstrap/1` | `crates/oo-jobs/migrations/0000_bootstrap_roles.sql` | `445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52` |
| `jobs-control/1` | `crates/oo-jobs/migrations/0001_jobs_control_expand.sql` | `b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75` |
| `jobs-event-project/1` | `crates/oo-jobs/migrations/0002_event_project_binding_expand.sql` | `4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee` |
| `oo.jobs.control-schema/v1` | `contracts/jobs/control-schema/jobs-control-v1.json` | `d352c86dbf2836360e541a3511af23674ce9692e687a6ac7041d99118dc8c2ca` |

The published follow-up is `contracts/jobs/event-project-binding/jobs-event-project-v1.json`. Clock remains `clock_timestamp()`. Database name in the fixture is `oo_control`.

## Checks

Baseline on archived commit `70a4e72c3ea698038e0bdd38df29e38c01dfd24d`, Linux arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Attempt 1 omitted `contracts/` and the control-schema test exited 101 (`pre-smoke/docker-run.exit`). The replacement snapshot includes `contracts`. `baseline/docker-run.exit` is 0, `worst.exit` is 0, inspect `Privileged=false`, `OOMKilled=false`. That run executed the committed `oo-jobs` tests only.

Host `rustc 1.98.1 (48a229cea 2026-09-01)`, `aarch64-apple-darwin`. `cargo fetch --locked`, `cargo test --locked --offline -p oo-jobs`, and `cargo clippy --locked --offline -p oo-jobs --all-targets -- -D warnings` exited 0 (`host/console.log`). Integration tests: `control_schema_files_match_the_published_contract` and `event_project_files_match_the_published_contract`.

Implementation overlay: `git archive HEAD` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `.cargo`, and `contracts`, then the uncommitted event-binding files. Same image pin `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `linux/docker-run.exit` is 0. `fetch.exit`, `test.exit`, `clippy.exit`, and `worst.exit` are 0. Inspect `Privileged=false`, cap drop `ALL`, port bindings `{}`. The same two integration tests passed inside the container. `airgap_claim=no`.

PostgreSQL fixture image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1)`, container `oo-a075-pg1710-jobs`, network `oo-a075-pg-net`, memory 2g, pids 512, privileged false, cap drop `ALL` plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`, no published port. Password file mode `0600` via `POSTGRES_PASSWORD_FILE`. Evidence has the variable name and not the password. Final harness line is `fixture_result=pass`.

The fixture roles `oo_a075_login` and `oo_a075_owner_login` are `LOGIN` members. `whoami` printed `oo_a075_login|oo_a075_login`. Product roles stay `NOLOGIN`.

Observed on both the upgraded database and the fresh database, unless a row says otherwise:

| Case | SQLSTATE | Durable result |
|---|---|---|
| Preexisting cross-project event, upgrade | `OO004` | Snapshot and `jobs-control/1` rows unchanged. Version absent `true\|true\|0\|0\|0`. |
| Forced failure after applying `0002` | `OO003` | Snapshot unchanged. Version absent. |
| Repeat before the upgrade row exists | `OO002` | Data snapshot unchanged. |
| Legal upgrade | | `ctid` and `xmin` snapshot of tasks, events, schedule, occurrence, and lease unchanged. `jobs-bootstrap/1` and `jobs-control/1` rows unchanged. Ready `t`. |
| Fresh `0000`+`0001`+`0002` in one transaction | | Binding present. |
| Repeat with the same three checksums | | Constraint OID and migration rows unchanged. SQL files not executed again. |
| Repeat with a zero event checksum, then a zero expand checksum | `OO001` | Migration rows unchanged. |
| Ready with the release checksums | | `t`. Zero event checksum: `f`. `jobs-control/1` ready probe stays `t`. |
| Runtime member replays `0002` inside a transaction | `42501` | Constraint OID unchanged. |
| Second migrator while the advisory lock is held | `55P03` | Later repeat matches the original migration rows. |
| Same-project direct insert | | Commits. Task generation and lease owner stay put. |
| Queued, running, succeeded | | `3\|true`. Every event project matches the task. |
| Missing task | `23503` | Constraint `task_events_task_id_fkey`. Note count 0. |
| Cross-project insert, including `SET CONSTRAINTS ALL DEFERRED` | `23503` | Constraint `task_events_task_project_fkey`. Note count 0. |
| Runtime update of an event, update of `tasks.project_id`, drop constraint, disable trigger, `session_replication_role`, `SET ROLE oo_jobs_owner` | `42501` | |
| Owner update of an accepted event | `23514` | `task event is immutable`. |
| Superuser disables only `tasks_enforce_update` and updates `project_id` | `23503` | `task_events_task_project_fkey`. The script does not commit. Project stays P1. Trigger returns to `O`. |
| Owner drops the foreign key | | Succeeds inside `BEGIN` and `ROLLBACK`. Constraint OID remains. |
| Concurrent heartbeat holds the task row | | Same-project insert commits. Cross-project insert `23503` on `task_events_task_project_fkey`. Project stays P1. Cross note count 0. |
| Concurrent `FOR UPDATE` then project change | `23514` on the holder, `23503` on the cross insert | Project stays P1. Cross note count 0. |

Earlier fixture attempts are kept. Attempt 1 stopped because `boolean::text` is `true`. Attempt 2 stopped because a schedule id contained `s`. Attempt 3 stopped because PostgreSQL 17 prints `ON UPDATE RESTRICT` before `ON DELETE RESTRICT`. Attempt 4 stopped because a bare `LOCK TABLE` outside a transaction is `25P01` for every role. The passing run replays the release file inside a transaction. Product SQL did not change between those attempts. `0002` stayed `4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee`.

## Still open

Claim and compare-and-swap, the 30 second lease default, the 10 second heartbeat, bootstrap checksum trust, an adopted `SECURITY DEFINER` function, and the target-database fence are separate work. Backup, restore, a 100-worker run, the queue driver, and the rest of SP02 are not accepted here. Review of this subitem follows.

## Controls

No root `Cargo.toml`, `Cargo.lock`, or other manifest edit. No other module edit. a073 remains the main root dependency owner. No new worktree. `0000`, `0001`, and `jobs-control-v1.json` bytes match the pins above. a068 and a064 evidence was not rewritten. No push, pull request, or customer write.

Builders: one own rust container at a time. `builders-before.txt` was empty for the implementation run. One own PostgreSQL 17.10 container. No host network, no socket mount, no host home mount, no published `5432`, `no-new-privileges`, finite memory, cpu, and pid caps. No `docker system prune`. After the logs and inspect files were on disk, this task removed `oo-a075-rust194-baseline`, `oo-a075-rust194-baseline2`, `oo-a075-rust194-impl`, `oo-a075-pg1710-jobs`, networks `oo-a075-baseline-net` and `oo-a075-pg-net`, volumes `oo-a075-baseline-cargo`, `oo-a075-baseline-target`, `oo-a075-impl-target`, and `oo-a075-pg-data`, and `/tmp/oo-a075-eef831aa-dba4-4af2-a78b-541bc9e0a075`. `oo-a072-pg1710-7c1a0e2b-4f63-4b0a-9c2d-11a072fence01` was still running after that removal.
