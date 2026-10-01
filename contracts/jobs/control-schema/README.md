# oo.jobs.control-schema/v1

Version `jobs-control/1`, phase `expand`. The SQL lives in `crates/oo-jobs/migrations`. Checksums in `jobs-control-v1.json` are SHA-256 of those files. This contract does not start a driver and does not accept AT-CTL-01 or AT-CTL-02.

## Inputs

- Control database. The fixture name is `oo_control`. The bootstrap SQL uses `current_database()`.
- Installer role that can create `NOLOGIN` roles. The fixture installer is a local superuser and is not a product runtime role.
- `0000_bootstrap_roles.sql`, then `0001_jobs_control_expand.sql` as `oo_jobs_owner`.
- Applicator checksum variables, 64 lowercase hex characters, for `jobs-bootstrap/1` and `jobs-control/1`.

## Interfaces

| Object | Interface |
|---|---|
| `oo_jobs_owner` | Schema owner. Runs the expand file. Not granted to the runtime. |
| `oo_jobs_runtime` | Internal jobs principal. Queue insert, claim-shaped update, schedule, occurrence, and control-lease updates. |
| `jobs.tasks` | REQ-CTL-04 columns. Unique `(project_id, dedupe_key)`. |
| `jobs.schedules` | Next fire time and immutable config digest. |
| `jobs.schedule_occurrences` | Primary key `(schedule_id, scheduled_at)`. |
| `jobs.leases` | Control-plane target key. `target_fence_certified` is false. |
| `jobs.task_events` | Append-only transition log. Runtime insert is possible. Update and delete are not. |
| `jobs.dead_letters` | `security_invoker` view of failed tasks. |
| `jobs.task_audit` | `security_invoker` view of the event log. |
| `jobs.schema_migrations` | Version, checksum, phase, filename, `applied_at`. |
| `jobs.migration_lock` | Singleton row. Advisory key `6401202601`. |
| `jobs.service_readiness` | `schema_version_present` for `jobs-control/1` expand. Checksum comparison is external. |

Clock authority is `clock_timestamp()`, not a client clock and not `now()` captured at transaction start.

## Outputs

A migrated database has both bookkeeping rows, the singleton lock row, and the grants in the expand file. Ready for a dependent process means `schema_version_present` is true and the stored checksum equals the release file. A checksum mismatch, a missing row, or a rolled-back migration is not ready.

Duplicate application does not create a second copy of the tables. A second `(project_id, dedupe_key)` insert returns unique violation `23505` and leaves the stored payload in place. The same dedupe key under another `project_id` is a second task.

## Backup marker

Durable tables: `jobs.schema_migrations`, `jobs.migration_lock`, `jobs.tasks`, `jobs.schedules`, `jobs.schedule_occurrences`, `jobs.leases`, `jobs.task_events`. The version marker is `jobs.schema_migrations.version = 'jobs-control/1'` together with its checksum. This contract does not record a backup or a restore.

## Left to SP02-W01-b and SP04

The queue driver, `FOR UPDATE SKIP LOCKED` claim loop, 30 second lease, 10 second heartbeat, cancel and retry policy, `NOTIFY` plus polling, worker CAS, and the target-database fence are not this schema. `jobs.leases` must not be treated as that fence.
