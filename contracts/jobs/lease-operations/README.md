# oo.jobs.lease-operations/v1

Expand version `jobs-lease-operations/1`. The SQL file is `crates/oo-jobs/migrations/0003_job_lease_operations_expand.sql`. Its SHA-256 is in `jobs-lease-operations-v1.json`. This follow-up does not replace `jobs-control/1` or `jobs-event-project/1` and does not change `0000_bootstrap_roles.sql`, `0001_jobs_control_expand.sql`, `0002_event_project_binding_expand.sql`, or their published contracts.

`jobs-control/1` stores the queue columns and the transition trigger. `jobs-event-project/1` binds an event row to its task project. Neither version installs claim, heartbeat, completion, or a fixed 30 second lease. This version adds those database functions. The Rust driver in `crates/oo-jobs/src/lib.rs` is still unimplemented.

## Who may call

`oo_jobs_runtime` is the trusted control principal for every project. It is not an Agent credential. `project_id` is a minimum filter on the row the function touches. It is not authentication and it is not project RBAC. Gateway and management authorization stay outside this schema. The owner and the migrator stay privileged. A migration run as the runtime raises `OO013`.

The seven functions are `SECURITY DEFINER`, owned by `oo_jobs_owner`, with `search_path = pg_catalog, jobs`. `PUBLIC EXECUTE` is revoked. `EXECUTE` is granted only to `oo_jobs_runtime`. Internal helpers `_require_lease_install`, `_lease_token`, and `_lease_result` are `SECURITY INVOKER` and are not executable by `PUBLIC` or by the runtime. The migration refuses `OO014` when any of those names already exist, so a planted definer is not adopted and not replaced.

`jobs-control/1` readiness does not authorize these functions. `jobs._require_lease_install` raises `OO015` until a `jobs-lease-operations/1` expand row with a 64-hex checksum is committed. That check does not open the release file.

## Ready means both

The applicator hashes the release file and stores that digest in the same transaction as the SQL. `jobs.lease_operations_readiness.schema_version_present` is true when the version row exists. A dependent process is ready only when that flag is true and the stored checksum equals the file SHA-256. The SQL will store a caller-supplied 64-hex lie and the view will still say present. Comparing the file bytes is the applicator's job. A068-F3 stays open.

## Calls

Payload is `text` so a bad JSON literal becomes static `OO010` inside the function. Stored payload is `jsonb`. It must be an object of at most 16384 octets. Top-level keys `password`, `passwd`, `secret`, `credential`, `credentials`, `api_key`, `access_token`, `private_key`, and `authorization` are rejected. Nested values and renamed keys are not scanned. Payload is opaque bounded metadata, not a credential channel.

`enqueue` inserts a queued task, or returns `replayed` when the same project and dedupe key already have the same kind, payload version, payload, config version, artifact digest, and max attempts. `run_at` is not part of that identity and is not updated on replay. A different identity raises `OO011` and writes nothing. The same dedupe key in another project is a second task. The function does not `COMMIT`. A caller can enqueue in the same transaction as its own business writes.

`claim` takes one due row in the project whose kind is in the list (1 to 64 distinct kinds) for a bounded ASCII worker id. Order is `run_at`, `id`, with `FOR UPDATE SKIP LOCKED`. The lease deadline is `clock_timestamp()` plus 30 seconds. There is no caller TTL and no function that backdates a deadline, forces a generation, or skips `max_attempts`. A future `run_at` is not selected. An empty selection returns status `none`. No `NOTIFY` is required to keep the row claimable.

An expired `running` row is not resumed on the old attempt. Under the row lock the function moves it to `queued`, clears the lease, and keeps attempt and generation. The existing trigger then accepts `queued` to `running` with attempt + 1 and generation + 1. The same worker id after a crash still receives a new generation. At the attempt cap the expired row becomes `failed` with static `last_error` `lease_expired_attempt_cap` and a cleared lease. A terminal row cannot be resurrected. At most 8 such cap finalizations happen in one call; the call then returns `none` even if a later due row exists. The next call continues. Generation `9223372036854775807` raises `OO012` before mutation and aborts the call, including cap finalizations already staged in that transaction. A max-generation due row at the head of a project and kind stays selected until an owner moves it out of the due set.

`heartbeat` sets `lease_until` to `clock_timestamp()` plus 30 seconds only when the row is `running`, the project, task, worker, and generation match, and `lease_until` is still after `clock_timestamp()`. An expired lease returns `lost_lease` and is not revived. A pure deadline change does not append `task_events`. The jobs-control/1 audit trigger records state, attempt, generation, owner, and `last_error`.

`complete_success` and `complete_failure` require that same active unexpired binding, then move the row to `succeeded` or `failed` and clear owner and lease. `complete_failure` stores an allowlisted kind: `worker_failed`, `payload_rejected`, `dependency_unavailable`, or `external_timeout`. Other text raises `OO010`. The audit trigger runs in the same transaction. Rollback keeps both the task and the event.

`retry_failure` requires the active binding. Attempt increases on the next claim, not here. `run_at` becomes `clock_timestamp()` plus a server delay: 1, 2, 4, 8, 16, or 32 seconds for attempt 1 through 6, and 60 seconds from attempt 7. The shift is only used for attempt 1..6. Docs/17 jitter is not applied. At the attempt cap the row becomes `failed` with `last_error` `attempt_exhausted`.

`cancel` moves `queued` to `cancelled` when the generation matches. The worker id is format-checked only, because a queued row has no owner. It moves `running` to `cancelled` when worker and generation match, including after `lease_until`. The running result returns the owner, generation, and deadline that were cleared. It does not terminate a target connection. A second cancel returns `terminal`.

Repeated completion returns `terminal`. This version does not add a terminal owner column and does not invent a matching token, so it does not return `already_applied`. Any generation presented after the row is terminal is `terminal`, not success. `lost_lease` is the typed miss for a stale owner, a stale generation, or an expired lease. No status means a target write was proved.

The token is `jlt1.` plus unauthenticated base64 of task, project, owner, generation, and the UTC deadline. Callers do not present it back. Compare-and-swap uses the explicit columns.

Nulls, bounds, and malformed JSON raise `OO010` with static text. Messages do not echo the payload, the worker id, or the SQL text.

## Grants after install

Direct `INSERT`, `UPDATE`, and `DELETE` on `jobs.tasks` and `jobs.task_events` are revoked from `oo_jobs_runtime`, including the event id sequence. `SELECT` remains. Schedule and `jobs.leases` column grants from `jobs-control/1` stay, because this version has no replacement API for them. `target_fence_certified` stays false and the runtime still has no `UPDATE` on that column. A control lease held by the runtime does not certify a target advisory fence. `jobs.service_readiness` still reports only `jobs-control/1`.

## Left open

The a079 cycle remains: `SHARE ROW EXCLUSIVE` on `task_events` then `tasks` can deadlock (`40P01`) with a claim that updates `tasks` and then inserts an event. This DDL does not change that lock order. The runtime can still take advisory lock `6401202601` (A068-F4). A superuser can still plant a definer outside the names this file checks (A068-F5 is only the adoption guard). Backup, restore, NOTIFY, the scheduler, the Rust driver, and AT-CTL-01/02 are not accepted here. One hundred concurrent claim sessions exercise this primitive only.
