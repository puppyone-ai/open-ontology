# oo-jobs control schema, jobs-control/1

Expand-phase migration for the control-database job queue. This is the schema slice of SP02-W01. It is not the queue driver, not AT-CTL-01, and not AT-CTL-02.

The normative column set is REQ-CTL-04. `jobs.tasks` has `id`, `project_id`, `kind`, `payload_version`, `payload`, `config_version`, `artifact_digest`, `dedupe_key`, `state`, `run_at`, `attempt`, `max_attempts`, `lease_owner`, `lease_until`, `generation`, `last_error`, `created_at`, and `updated_at`. States are `queued`, `running`, `succeeded`, `failed`, and `cancelled`. Doc 17 §3.1 is the same queue with the spec's project, digest, and timestamp columns added.

## Files

| Version | Phase | File | What it does |
|---|---|---|---|
| `jobs-bootstrap/1` | expand | `0000_bootstrap_roles.sql` | Creates `oo_jobs_owner` and `oo_jobs_runtime`. |
| `jobs-control/1` | expand | `0001_jobs_control_expand.sql` | Creates `jobs` and its tables, views, and grants. |
| `jobs-event-project/1` | expand | `0002_event_project_binding_expand.sql` | Binds `task_events.project_id` to the task. Published separately. |
| `jobs-lease-operations/1` | expand | `0003_job_lease_operations_expand.sql` | Adds the lease functions and revokes direct task and event writes. |

Checksums are SHA-256 of those file bytes. `0000` and `0001` are recorded in `contracts/jobs/control-schema/jobs-control-v1.json`. `0002` is recorded in `contracts/jobs/event-project-binding/jobs-event-project-v1.json`. `0003` is recorded in `contracts/jobs/lease-operations/jobs-lease-operations-v1.json`. The checksum is not inside the SQL file. A later edit of a published file is a new version, not a silent rewrite. `0000`, `0001`, and `0002` are unchanged by `0003`.

There is no migrate-phase or contract-phase file in this slice. REQ-OPS-06's expand, then migrate, then contract sequence stops after expand.

## Applicator

The Rust driver is not in this crate. `tests/pg_fixture.sh` is the reference applicator for `0000` and `0001`. `tests/lease_operations_live.rs` is the reference applicator for `0003`, including fresh install, upgrade, and repeat. Both pass fixed `psql -f` paths and hex checksum variables. They do not concatenate SQL text.

The applicator takes `pg_advisory_xact_lock(6401202601)` and, once `jobs.migration_lock` exists, `FOR UPDATE` on that row. The key is only the migration executor lock.

On an empty database the control-schema applicator runs `0000` and `0001` in one transaction and inserts those two `jobs.schema_migrations` rows. A second run with the same checksums locks, compares, and does not execute the files again. A different checksum raises `OO001` and does not change the schema. A failed file rolls back with the transaction. `jobs.service_readiness.schema_version_present` is true only when the `jobs-control/1` expand row exists. A caller still has to compare that row's checksum to the release file. The view does not open the file. `0002` and `0003` follow the same rule in their own contracts.

`jobs.schema_migrations` rejects update and delete. The runtime role cannot insert, update, or delete it.

## Roles

`oo_jobs_owner` is `NOLOGIN`, `NOSUPERUSER`, `NOCREATEDB`, `NOCREATEROLE`, `NOINHERIT`, `NOBYPASSRLS`, and `NOREPLICATION`. It owns the schema. The installer creates it. The migration does not grant that role to the runtime.

`oo_jobs_runtime` has the same login and privilege limits. It is the internal jobs principal. It is not an Agent credential, and it is not a control-management identity. A later cross-project control API uses a different role. That role is not created here.

`0001` grants the runtime insert of a queued task and update of the columns a claim, heartbeat, cancel, retry, or completion would touch, plus insert of `task_events`. It can insert schedule definitions, occurrence rows, and inactive lease rows, and it can update the lease owner, deadline, generation, and holder. It cannot create objects in `jobs`, alter or drop the schema, write migration bookkeeping, or update payload, config version, or artifact digest. `0003` revokes direct task and event DML. The schedule and `jobs.leases` column grants stay. See the lease-operations section below.

`PUBLIC` has no privilege on the schema, its tables, or its functions. `PUBLIC EXECUTE` on functions is a hard-wired global default. A per-schema `ALTER DEFAULT PRIVILEGES` entry can only add grants, so the migration revokes it globally for `oo_jobs_owner` and grants `EXECUTE` back to `oo_jobs_runtime` only for later functions in `jobs`. Tables and sequences have no hard-wired `PUBLIC` grant. A later table still needs an explicit runtime grant.

`0001` trigger functions are `SECURITY INVOKER`. `0001` installs no `SECURITY DEFINER` function and no extension. `0003` adds the seven definer functions in the lease-operations section.

## What the database enforces

- Task state, lease owner, and lease deadline have to match. `running` has an owner, a deadline, a positive generation, and an attempt. `queued` has neither owner nor deadline. `failed` has a bounded `last_error` and no lease. `succeeded` and `cancelled` have no lease and no error.
- `attempt` is between 0 and `max_attempts`. `max_attempts` is from 1 to 32.
- Generation does not decrease. A new non-null lease owner requires a new generation. A claim from `queued` to `running` also increments `attempt` by one. Terminal states do not leave. A queued row cannot jump to `succeeded` or `failed`. A retry back to `queued` is rejected once `attempt` has reached `max_attempts`.
- `(project_id, dedupe_key)` is unique. The same key in another project is a different task. A second insert does not overwrite payload, version, or digest. The later API reads the stored row and decides whether the call is an idempotent replay or a conflict. This schema has no `ON CONFLICT DO UPDATE`.
- `(schedule_id, scheduled_at)` is the occurrence key. Inserting that pair again conflicts. An occurrence row cannot be updated or deleted.
- `jobs.leases.exclusivity_domain` is `control_jobs`. `target_fence_certified` is false. An active control lease is not an Agent DB advisory lock and does not certify a target writer.
- Payload text is at most 16384 octets. `last_error` is at most 2048. Digests are 64 lowercase hex characters.
- `created_at` defaults to `clock_timestamp()`. Update triggers set `updated_at` from `clock_timestamp()`.

`jobs.dead_letters` is the `security_invoker` view of `state = 'failed'`. `jobs.task_audit` is the `security_invoker` view of `jobs.task_events`. Task insert and the tracked updates append an event. After `0001` alone, the runtime role can also insert an event itself. It cannot update or delete one. `0003` revokes that insert. The trigger does not prove which worker opened the session.

## Lease operations, jobs-lease-operations/1

`0003_job_lease_operations_expand.sql` installs owner-owned `SECURITY DEFINER` functions: `enqueue`, `claim`, `heartbeat`, `complete_success`, `complete_failure`, `retry_failure`, and `cancel`. `search_path` is `pg_catalog, jobs`. `PUBLIC EXECUTE` is revoked. Only `oo_jobs_runtime` can execute them. The file must run as `oo_jobs_owner` after the event-project foreign key exists. It refuses to adopt an existing function of the same name (`OO014`). It does not disable triggers, grant the owner role to the runtime, or rewrite task or event rows.

The lease is `clock_timestamp()` plus 30 seconds. Heartbeat extends that same fixed interval and does not accept a caller TTL. An expired lease is not revived. Claim uses `FOR UPDATE SKIP LOCKED` ordered by `run_at`, `id`. Expired running work is requeued under the lock and then claimed as a new attempt and generation, including when the worker id string is unchanged. At the attempt cap the expired row fails with `lease_expired_attempt_cap`. One call finalizes at most 8 such rows. Retry delay is server-side, base 1 second, cap 60 seconds, without jitter. The functions do not `COMMIT` and do not `NOTIFY`.

Direct runtime `INSERT`, `UPDATE`, and `DELETE` on `jobs.tasks` and `jobs.task_events` are revoked, along with the event id sequence. `SELECT` remains. Column grants on `jobs.schedules`, `jobs.schedule_occurrences`, and `jobs.leases` from `0001` remain. `target_fence_certified` stays constrained false, and the runtime has no update privilege on that column. `jobs.service_readiness` still reports only `jobs-control/1`. `jobs.lease_operations_readiness` reports this version. Ready for a dependent process is that view plus an external compare of the stored checksum to the file bytes. The SQL stores the checksum the applicator supplies.

`project_id` filters rows. It does not authenticate a project. The runtime remains a trusted all-project control role.

Repeated completion returns `terminal`. The schema has no terminal owner column, so this version does not return `already_applied`. A pure heartbeat does not append `task_events`.

The a079 deadlock between a `SHARE ROW EXCLUSIVE` lock on `task_events` then `tasks` and a claim that updates `tasks` then inserts an event is still possible (`40P01`). A068-F3, F4, and F5 stay open in the ways the lease-operations contract names. The Rust driver is not in `src/lib.rs`.

## API-only invariants that remain

These are still not closed by the functions:

- The runtime role can see every `project_id`. Row security is off. Project authorization for a human or an Agent is a later management identity.
- `NOTIFY` is not installed. A missed wake does not drop a due row. Claim scans.
- Retry jitter is not stored. The delay above is deterministic.
- No business table is in this schema. A caller may put its own business write and `enqueue` in one transaction. The function does not commit that transaction.
- The control lease does not take, and cannot record, a target-database fence. Target fencing stays with the loader connection in SP04.
- Schedule and control-lease column DML from `0001` is still granted. This version has no replacement API for those tables.

## Backup

Backup consumers need the seven durable tables listed in the control-schema contract, including `jobs.schema_migrations`. `jobs.service_readiness` still marks `jobs-control/1`. `jobs.lease_operations_readiness` marks `jobs-lease-operations/1`. Views are rebuilt by the migrations. This repository does not claim a backup or a restore was run.

## Tests

`tests/control_schema_contract.rs` and `tests/event_project_binding_contract.rs` check the published files. `tests/lease_operations_contract.rs` checks `0003` and its contract. `tests/pg_fixture.sh` applies `0000` and `0001`. `tests/lease_operations_live.rs` is ignored unless a caller asks for it, and it applies the lease functions on an owned PostgreSQL 17 fixture. None of these is the full AT-CTL-01 100-worker acceptance test. The live file's 100 sessions are the claim primitive only.
