# oo-jobs control schema, jobs-control/1

Expand-phase migration for the control-database job queue. This is the schema slice of SP02-W01. It is not the queue driver, not AT-CTL-01, and not AT-CTL-02.

The normative column set is REQ-CTL-04. `jobs.tasks` has `id`, `project_id`, `kind`, `payload_version`, `payload`, `config_version`, `artifact_digest`, `dedupe_key`, `state`, `run_at`, `attempt`, `max_attempts`, `lease_owner`, `lease_until`, `generation`, `last_error`, `created_at`, and `updated_at`. States are `queued`, `running`, `succeeded`, `failed`, and `cancelled`. Doc 17 §3.1 is the same queue with the spec's project, digest, and timestamp columns added.

## Files

| Version | Phase | File | What it does |
|---|---|---|---|
| `jobs-bootstrap/1` | expand | `0000_bootstrap_roles.sql` | Creates `oo_jobs_owner` and `oo_jobs_runtime`. |
| `jobs-control/1` | expand | `0001_jobs_control_expand.sql` | Creates `jobs` and its tables, views, and grants. |

Checksums are SHA-256 of those file bytes and are recorded in `contracts/jobs/control-schema/jobs-control-v1.json`. The checksum is not inside the SQL file. A later edit of either file is a new version, not a silent rewrite of `jobs-control/1`.

There is no migrate-phase or contract-phase file in this slice. REQ-OPS-06's expand, then migrate, then contract sequence stops after expand.

## Applicator

The Rust driver is not in this crate. Until it exists, `tests/pg_fixture.sh` is the reference applicator. It passes fixed `psql -f` paths and hex checksum variables. It does not concatenate SQL text.

The applicator takes `pg_advisory_xact_lock(6401202601)` and then `jobs.migration_lock` `FOR UPDATE`. The key is only the migration executor lock.

On an empty database it runs both files in one transaction and inserts the two `jobs.schema_migrations` rows. A second run with the same checksums locks, compares, and does not execute the files again. A different checksum raises `OO001` and does not change the schema. A failed file rolls back with the transaction. `jobs.service_readiness.schema_version_present` is true only when the `jobs-control/1` expand row exists. A caller still has to compare that row's checksum to the release file. The view does not open the file.

`jobs.schema_migrations` rejects update and delete. The runtime role cannot insert, update, or delete it.

## Roles

`oo_jobs_owner` is `NOLOGIN`, `NOSUPERUSER`, `NOCREATEDB`, `NOCREATEROLE`, `NOINHERIT`, `NOBYPASSRLS`, and `NOREPLICATION`. It owns the schema. The installer creates it. The migration does not grant that role to the runtime.

`oo_jobs_runtime` has the same login and privilege limits. It is the internal jobs principal. It is not an Agent credential, and it is not a control-management identity. A later cross-project control API uses a different role. That role is not created here.

The runtime role can insert a queued task and the columns a claim, heartbeat, cancel, retry, or completion updates. It can insert schedule definitions, occurrence rows, and inactive lease rows, and it can update the lease owner, deadline, generation, and holder. It cannot create objects in `jobs`, alter or drop the schema, write migration bookkeeping, or update payload, config version, or artifact digest.

`PUBLIC` has no privilege on the schema, its tables, or its functions. `PUBLIC EXECUTE` on functions is a hard-wired global default. A per-schema `ALTER DEFAULT PRIVILEGES` entry can only add grants, so the migration revokes it globally for `oo_jobs_owner` and grants `EXECUTE` back to `oo_jobs_runtime` only for later functions in `jobs`. Tables and sequences have no hard-wired `PUBLIC` grant. A later table still needs an explicit runtime grant.

Trigger functions are `SECURITY INVOKER`. No function is `SECURITY DEFINER`. No extension is installed.

## What the database enforces

- Task state, lease owner, and lease deadline have to match. `running` has an owner, a deadline, a positive generation, and an attempt. `queued` has neither owner nor deadline. `failed` has a bounded `last_error` and no lease. `succeeded` and `cancelled` have no lease and no error.
- `attempt` is between 0 and `max_attempts`. `max_attempts` is from 1 to 32.
- Generation does not decrease. A new non-null lease owner requires a new generation. A claim from `queued` to `running` also increments `attempt` by one. Terminal states do not leave. A queued row cannot jump to `succeeded` or `failed`. A retry back to `queued` is rejected once `attempt` has reached `max_attempts`.
- `(project_id, dedupe_key)` is unique. The same key in another project is a different task. A second insert does not overwrite payload, version, or digest. The later API reads the stored row and decides whether the call is an idempotent replay or a conflict. This schema has no `ON CONFLICT DO UPDATE`.
- `(schedule_id, scheduled_at)` is the occurrence key. Inserting that pair again conflicts. An occurrence row cannot be updated or deleted.
- `jobs.leases.exclusivity_domain` is `control_jobs`. `target_fence_certified` is false. An active control lease is not an Agent DB advisory lock and does not certify a target writer.
- Payload text is at most 16384 octets. `last_error` is at most 2048. Digests are 64 lowercase hex characters.
- `created_at` defaults to `clock_timestamp()`. Update triggers set `updated_at` from `clock_timestamp()`.

`jobs.dead_letters` is the `security_invoker` view of `state = 'failed'`. `jobs.task_audit` is the `security_invoker` view of `jobs.task_events`. Task insert and the tracked updates append an event. The runtime role can also insert an event itself. It cannot update or delete one. The trigger does not prove which worker opened the session.

## API-only invariants

These are not database constraints. A session that holds `oo_jobs_runtime` can do them today:

- Lease duration is not fixed at 30 seconds, and heartbeat is not fixed at 10 seconds. The fixture stores a 24 hour `lease_until`. Callers compare `lease_until` to `clock_timestamp()`. The database clock is the authority for that comparison. The column does not reject a client-chosen deadline.
- The runtime role can see and enqueue every `project_id`. Row security is off. Project authorization for a human or an Agent is a later management identity.
- The role can finish or reclaim any row whose next values are legal. It does not carry a worker id. The driver has to keep `WHERE lease_owner = $worker AND generation = $generation` on heartbeat, cancel, and completion. Omitting that predicate is possible with this credential.
- `NOTIFY` is not installed. A claim scan is `state = 'queued' AND run_at <= clock_timestamp()` with `FOR UPDATE SKIP LOCKED`, using `tasks_claim_idx`. That statement is not a stored function.
- Backoff and jitter are not stored. `run_at` is the next instant.
- No business table is in this schema. Putting a business write and the enqueue in one transaction is the driver's job once those tables exist.
- The control lease does not take, and cannot record, a target-database fence. Target fencing stays with the loader connection in SP04.

## Backup

Backup consumers need the seven durable tables listed in the contract, including `jobs.schema_migrations`. The version marker is the `jobs-control/1` row and its checksum. Views are rebuilt by the migration. This repository does not claim a backup or a restore was run.

## Tests

`tests/control_schema_contract.rs` checks the file text and the published checksums. `tests/pg_fixture.sh` applies the files to an owned PostgreSQL 17 fixture. Neither one is the 100-worker acceptance test.
