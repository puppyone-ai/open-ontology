# oo.jobs.event-project-binding/v1

Expand version `jobs-event-project/1`. The SQL file is `crates/oo-jobs/migrations/0002_event_project_binding_expand.sql`. Its SHA-256 is in `jobs-event-project-v1.json`. This follow-up does not replace `jobs-control/1` and does not change `0000_bootstrap_roles.sql`, `0001_jobs_control_expand.sql`, or `jobs-control-v1.json`.

`jobs-control/1` lets a runtime session insert `jobs.task_events` with a `project_id` other than the task's. A068-F2 recorded that gap. This version closes the equality gap before a project-scoped reading of the event log. It does not mean the original schema review was wrong to approve SP02-W01-a.

## Database rule

The migration adds non-deferrable `UNIQUE (id, project_id)` on `jobs.tasks`, then `FOREIGN KEY (task_id, project_id) REFERENCES jobs.tasks (id, project_id) ON DELETE RESTRICT ON UPDATE RESTRICT`. The unique constraint is first because the foreign key needs that column order. Locks are `task_events`, then `tasks`.

A preexisting row whose project does not equal its task raises `OO004` and the transaction rolls back. The file does not update, delete, or insert event rows, and it does not invent a repair plan. Matching historical rows stay as they are. Append-only triggers from `jobs-control/1` stay in place. The only new grant is `SELECT` on `jobs.event_project_readiness`.

A direct insert may still carry a caller-chosen note, generation, lease owner, or state pair. Those fields are not a worker identity. This is not a tamper-proof audit. Row security stays off, so the runtime principal can still see every project.

## Applicator

`crates/oo-jobs/tests/event_project_fixture.sh` is the reference applicator. Fresh install, repeat, and upgrade are separate scripts. All three use `pg_advisory_xact_lock(6401202601)` and fixed `psql -f` paths. The shell hashes the release files and passes those digests. Repeat with a different digest raises `OO001` and does not run the SQL again. Fresh install still stores the digest the applicator supplies. Comparing that stored digest to the file remains an external ready check. That bootstrap trust gap is unchanged.

Ready for this version means `jobs.event_project_readiness.schema_version_present` is true, its checksum equals this file, and the `jobs-control/1` checksum still equals `0001_jobs_control_expand.sql`. `jobs.service_readiness` still reports only `jobs-control/1`.

## Left open

Claim and compare-and-swap, the 30 second lease, the 10 second heartbeat, bootstrap checksum trust, an adopted definer function, and the target-database fence are not this migration. Backup, restore, a 100-worker run, the queue driver, and the rest of SP02 are not accepted here.
