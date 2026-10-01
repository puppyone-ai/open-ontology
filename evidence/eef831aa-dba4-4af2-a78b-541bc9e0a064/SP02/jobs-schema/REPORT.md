# SP02-W01-a jobs control schema

Schema slice only. `jobs-control/1` is applied and checked on an owned PostgreSQL 17.10 fixture. This does not finish SP02-W01, Phase SP02, AT-CTL-01, or AT-CTL-02. No queue driver, no 100-worker run, no backup or restore, and no reviewer approval.

The commit that contains this file is local. Its SHA is recorded in the session handoff. The SHA cannot be written into this file, because the hash covers these bytes.

## Version and checksums

| Version | Phase | File | SHA-256 |
|---|---|---|---|
| `jobs-bootstrap/1` | expand | `crates/oo-jobs/migrations/0000_bootstrap_roles.sql` | `445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52` |
| `jobs-control/1` | expand | `crates/oo-jobs/migrations/0001_jobs_control_expand.sql` | `b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75` |

Published copy: `contracts/jobs/control-schema/jobs-control-v1.json`. Migration executor key: `6401202601`. Clock: `clock_timestamp()`. Database name in the fixture: `oo_control`.

## Inputs

- Installer that can create `NOLOGIN` roles. The fixture installer is `oo_fixture_admin`, superuser of the ephemeral container only. It is not a product role.
- `0000_bootstrap_roles.sql`, then `0001_jobs_control_expand.sql` as `oo_jobs_owner`.
- Two 64-character lowercase hex checksums, supplied as `psql` variables. The applicator uses fixed `psql -f` paths. It does not concatenate SQL.

## Interfaces

| Object | Interface |
|---|---|
| `oo_jobs_owner` | `NOLOGIN` schema owner. Not granted to the runtime. |
| `oo_jobs_runtime` | Internal jobs principal. Not an Agent credential. |
| `jobs.tasks` | REQ-CTL-04 columns. Unique `(project_id, dedupe_key)`. |
| `jobs.schedules` | Immutable config digest and next fire time. |
| `jobs.schedule_occurrences` | Primary key `(schedule_id, scheduled_at)`. |
| `jobs.leases` | `exclusivity_domain = control_jobs` and `target_fence_certified = false`. |
| `jobs.task_events` | Append-only log. Runtime may insert. Update and delete are rejected. |
| `jobs.dead_letters` | `security_invoker` view of `state = failed`. |
| `jobs.task_audit` | `security_invoker` view of `jobs.task_events`. |
| `jobs.schema_migrations` | Version, checksum, phase, filename, `applied_at`. |
| `jobs.migration_lock` | Singleton row `id = 1`. |
| `jobs.service_readiness` | `schema_version_present` for `jobs-control/1` expand. |

## Outputs

A migrated database has both bookkeeping rows, the lock row, and the grants in the expand file. Ready for a dependent process means `schema_version_present` is true and the stored checksum equals the release file. Repeat application with the same checksums does not execute the files again. A different checksum raises `OO001` and leaves the rows in place. A statement that raises inside the migration transaction rolls back. The follow-on probe then still shows `jobs-bootstrap/1` and `jobs-control/1`, with `jobs._rollback_probe` absent and `jobs-control/2` absent.

Same `dedupe_key` in two projects is two tasks. A second insert of the same `(project_id, dedupe_key)` raises `23505` and does not overwrite payload, version, or digest. The later API decides replay versus conflict.

## Backup marker

Consumers must include `jobs.schema_migrations`, `jobs.migration_lock`, `jobs.tasks`, `jobs.schedules`, `jobs.schedule_occurrences`, `jobs.leases`, and `jobs.task_events`. The version marker is `jobs.schema_migrations.version = 'jobs-control/1'` together with its checksum. Views are rebuilt by the migration. Backup and restore were not run.

## Fixture

Image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, PostgreSQL `17.10 (Debian 17.10-1.pgdg12+1)`, container `oo-a064-pg1710-jobs`, label `oo.owner=a064`, run `eef831aa-dba4-4af2-a78b-541bc9e0a064`. No published host port. Password file mode `0600`. Evidence under this directory has zero copies of that password.

Final applicator exit is 0. Log: `pg/harness.log`. SQL archive is the migration files. Role attributes: `pg/archive_roles.stdout` (`rolsuper`, `rolinherit`, `rolcreaterole`, `rolcreatedb`, `rolcanlogin`, `rolbypassrls`, `rolreplication` all false for both roles). Effective grants: `pg/archive_grants.stdout`.

Runtime column grants from that archive:

- `tasks` INSERT: `id`, `project_id`, `kind`, `payload_version`, `payload`, `config_version`, `artifact_digest`, `dedupe_key`, `max_attempts`, `run_at`. UPDATE: `state`, `run_at`, `attempt`, `lease_owner`, `lease_until`, `generation`, `last_error`, `updated_at`. SELECT on all 18 task columns.
- `schedules` INSERT of identity, kind, config, digest, prefix, and `next_run_at`. UPDATE of `next_run_at`, `enabled`, `updated_at`.
- `schedule_occurrences` INSERT of `schedule_id`, `scheduled_at`, `project_id`, `task_id`.
- `leases` INSERT of `id`, `project_id`, `target_key`. UPDATE of `owner`, `lease_until`, `generation`, `holder_task_id`, `updated_at`. No INSERT or UPDATE of `target_fence_certified`.
- `task_events` SELECT and INSERT. `schema_migrations` SELECT. The three views SELECT.
- Schema `jobs` ACL: owner `UC`, runtime `U`. `PUBLIC` is absent. Routine `EXECUTE` is owner and runtime only.

`jobs.migration_lock` does not appear in the runtime grant list. Runtime has no schema `CREATE`.

Preserved applicator failures, then the fix, then the passing run:

| Attempt | Exit | What the log shows |
|---|---|---|
| Privilege assert | 3 | `future object leaked a public grant`. A per-schema `REVOKE EXECUTE` stores nothing, because `PUBLIC EXECUTE` is hard-wired globally. Preserved in `pg/attempt1-privilege-fail/`. |
| `set_owner` | 1 | `SET ROLE` from the superuser session still succeeded. The fixture now uses `SET SESSION AUTHORIZATION oo_jobs_runtime`, which returns `42501`. Preserved in `pg/attempt2-set-owner-session-user/`. |
| `drop_nobody` | 3 | Role remained because of grants in `oo_control`. Drop now runs `DROP OWNED` in that database. Preserved in `pg/attempt3-drop-nobody-dependents/`. |
| Final | 0 | `fixture_result=pass`. |

Passing checks on that final run: fresh apply, repeat with identical relation counts and migration rows, `OO001` on 64 zero checksum bytes with rows unchanged, readiness `t` for the real checksum and `f` for the zero checksum, privilege assert, legal queue flow, 40 negative statements, snapshot diff empty, direct audit insert counted as 1, concurrent lock `55P03`, failed fresh migration `OO003` with probe `t|t`, failed follow-on rolled back.

Negative SQLSTATEs on the final run, each after a legal control row where the statement needed one:

| Case | SQLSTATE |
|---|---|
| `bad_state`, `queued_with_lease`, `generation_down`, `payload_bound`, `bad_digest`, `queued_to_succeeded`, `terminal_reopen`, `heartbeat_owner`, `exhausted_retry`, `owner_max_zero`, `owner_max_high`, `owner_attempt_over`, `owner_fence`, `lease_generation_down`, `occurrence_project`, `running_without_lease`, `last_error_bound` | `23514` |
| `dedupe_conflict`, `dedupe_same`, `occurrence_rollback` | `23505` |
| `digest_update`, `payload_update`, `config_update`, `fence_column`, `drop_schema`, `create_table`, `create_public_table`, `create_role`, `set_owner`, `alter_table`, `insert_migration`, `update_migration`, `delete_migration`, `lock_migration`, `other_namespace`, `create_function`, `create_extension`, `delete_task`, `event_update`, `nobody_view` | `42501` |

`nobody_view` is `42501` on `jobs.tasks` with `security_invoker`. A definer view would have returned the row. `audit_direct_insert=allowed` is the recorded gap: runtime can insert `jobs.task_events` and cannot update or delete them.

## Linux and MSRV

Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `RUSTUP_TOOLCHAIN=1.94.0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, commit `4a4ef493e3a1488c6e321570238084b38948f6db`. Workspace `rust-toolchain.toml` is newer. The container overrides it. One builder at a time. Owned cargo and target volumes. No dual-arch matrix.

| Run | Exit | Result |
|---|---|---|
| Smoke attempt 1 | 1 | `cargo 1.94.0 fetch` rejects `-p`. Preserved in `smoke/attempt1-*`. |
| Smoke attempt 2, before these edits | 0 | `cargo fetch --locked` then `cargo test -p oo-jobs --locked --offline`. 0 tests. `Finished` in 21.97s. `smoke/attempt2-*`. Snapshot `67795641ecb04855af7720c6a8c81e41e542bf92edc2f92a7df5074f5bcc7726`, 103 files. |
| Contract attempt 1 | 101 | `cargo fetch --locked --offline` refused to rewrite `Cargo.lock`. The live lock was moving and is not an a064 file. `smoke/contract-attempt1-*`. |
| Contract attempt 2 | 0 | Sealed pre-edit snapshot plus `crates/oo-jobs` and `contracts/jobs/control-schema`. `cargo test -p oo-jobs --locked --offline`. `control_schema_files_match_the_published_contract` passed. 1 passed, 0 failed. `smoke/contract-attempt2-*`. Tree `46a3d7358668b567618ee8273703dc5c5d7c5142ab5476338422d344f21b2057`, 173 files. |

## API-only invariants

These were not given a database constraint. A session that already holds `oo_jobs_runtime` can do them:

- Lease length and heartbeat interval. The fixture stores a 24 hour `lease_until`. Callers compare it to `clock_timestamp()`.
- Any `project_id`. Row security is off. A later cross-project call uses a control management identity. That role is not created here.
- Finish or reclaim a row without `lease_owner` and `generation` in the `WHERE` clause. The driver has to send that predicate.
- Insert an audit event directly.
- Skip `NOTIFY`, backoff storage, and a stored claim function. The claim shape is `state = queued AND run_at <= clock_timestamp()` with `FOR UPDATE SKIP LOCKED` on `tasks_claim_idx`.

`jobs.leases` does not take an Agent DB advisory lock and does not certify a target writer.

## Left for SP02-W01-b and SP04

SP02-W01-b is the Rust queue driver: claim, heartbeat, cancel, retry, worker compare-and-swap, and the 30 second / 10 second policy. Dependencies can be added in that task. This crate still has no `sqlx` driver.

SP04 owns the target-database fence. The control schema cannot certify it.

No production or customer source was written. `Cargo.toml`, `Cargo.lock`, `src/lib.rs`, specs, plans, and ledgers were not edited.
