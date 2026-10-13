# job-worker SP02-W01 runtime

Frozen before the driver. This crate is the Rust jobs runtime for one control database. It calls the published lease functions. It does not replace them and it does not edit `crates/oo-jobs`, `crates/oo-control`, or `contracts/jobs`.

Parent SP02-W01, AT-CTL-01, AT-CTL-02, target fencing, backup, and release stay unsigned.

## Read-only primitives

The applicator hashes these files and refuses to run when the bytes differ.

| File | Version | SHA-256 |
|---|---|---|
| `0000_bootstrap_roles.sql` | `jobs-bootstrap/1` | `445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52` |
| `0001_jobs_control_expand.sql` | `jobs-control/1` | `b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75` |
| `0002_event_project_binding_expand.sql` | `jobs-event-project/1` | `4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee` |
| `0003_job_lease_operations_expand.sql` | `jobs-lease-operations/1` | `5cb2e9030a479903ecc57ce32fc3ee851f00d146e02ab3902fcbe7ab09501034` |

Calls, all `SECURITY DEFINER`, all committed by the driver in a short transaction:

| Function | Use |
|---|---|
| `jobs.enqueue(uuid, text, int, text, text, text, text, timestamptz, int)` | Register a task. Same project and dedupe identity returns `replayed`. |
| `jobs.claim(uuid, text[], text)` | One due row. `FOR UPDATE SKIP LOCKED`. Lease is server `clock_timestamp()` plus 30 seconds. |
| `jobs.heartbeat(uuid, uuid, text, bigint)` | Extend that same 30 seconds. No caller TTL. |
| `jobs.complete_success(uuid, uuid, text, bigint)` | CAS owner and generation. |
| `jobs.complete_failure(uuid, uuid, text, bigint, text)` | Allowlisted kind only. |
| `jobs.retry_failure(uuid, uuid, text, bigint, text)` | Server delay. No jitter argument. |
| `jobs.cancel(uuid, uuid, text, bigint)` | Queued generation match, or running owner and generation. |

Statuses the driver accepts: `enqueued`, `replayed`, `claimed`, `none`, `extended`, `succeeded`, `failed`, `retried`, `cancelled`, `lost_lease`, `terminal`. `already_applied` is not a success path.

Ready means `jobs.lease_operations_readiness.schema_version_present` and all four stored checksums equal the file hashes above. A stored checksum alone is not readiness.

## Driver API

`TaskRequest` fields: `project_id`, `dedupe_key`, `units` (1..=32), `fail_attempts` (0..=32), `stop_after` (optional 0..=32), `terminal_failure`, `max_attempts` (1..=32), `run_delay_seconds` (0..=120).

Canonical payload text, key order fixed:

```text
{"v":1,"units":N,"fail_attempts":F}
{"v":1,"units":N,"fail_attempts":F,"stop_after":S}
{"v":1,"units":N,"fail_attempts":F,"terminal_failure":true}
```

`artifact_digest` is the SHA-256 of those bytes. `config_version` is `jw-payload-1`. `payload_version` is 1. Kind is `pg_effect`.

`stop_after` is honored only while `generation` is 1. A later generation continues until `units`. The payload is not rewritten.

Heartbeat decision uses database time. The first beat is due when `clock_timestamp() >= claimed_at + 10 seconds`. A later beat is due when `clock_timestamp() >=` the previous beat `observed_at + 10 seconds`. The 10 is `HEARTBEAT_SECONDS`. Claim and heartbeat calls do not receive a lease length.

`pg_notify('oo_jw_wake', project_id)` runs in the same transaction as a committed enqueue. Workers `LISTEN` on that channel and also claim on a one-second poll. A notification does not change a task row.

Schedule registration inserts `jobs.schedules` with the published column grant and stores the payload in `jw.schedule_payloads` because `jobs.schedules` has no payload column. Firing calls `jobs.enqueue` and inserts `jobs.schedule_occurrences` in one transaction. The occurrence primary key is what makes a repeated tick one task.

Side effects are rows in `jw.effects`, one committed transaction each, gated by the live `jobs.tasks` lease. `pg_current_xact_id()` is stored on the row. A stale owner inserts nothing.

## Acceptance that this crate must run

Against one private PostgreSQL 17.10, in one process with 100 runtime sessions:

1. 100 due tasks are claimed by 100 workers before any of those tasks completes. Peak `running` for `bulk-%` is 100. Each then commits one effect. One hundred distinct claim owners. One success event each.
2. A second enqueue of the same bulk identity is `replayed` and does not add a task.
3. A business insert and `jobs.enqueue` in one transaction, rolled back, leave no task and no intent row.
4. `NOTIFY` while nobody is listening leaves the task `queued`. A later poll claims it. One effect.
5. Two concurrent fires of one schedule tick leave one occurrence, one task, and one effect.
6. `fail_attempts = 1` and `max_attempts = 3` retries once, then commits one effect and succeeds on attempt 2.
7. `max_attempts = 1` with `fail_attempts = 1` becomes `failed` / `attempt_exhausted`, is visible in `jobs.dead_letters`, and has no effect.
8. `terminal_failure` becomes `failed` / `worker_failed` with no effect.
9. A payload with `units = 0` becomes `failed` / `payload_rejected` with no effect.
10. A queued cancel ends `cancelled` with no effect.
11. A running cancel of the current owner and generation ends `cancelled` with no effect. A later complete is not `succeeded`.
12. The checkpoint task commits 2 effects, then the process is killed. The old owner cannot complete after `lease_until <= clock_timestamp()`. Restart claims a new generation, commits the remaining effects, and succeeds. Effect count is 4. Distinct transaction ids equal 4. The first generation's complete is not `succeeded`.
13. Measured `lease_until - claimed_at` on that task is from 25 to 30 seconds. The first heartbeat is at least 10 seconds after `claimed_at`, and that heartbeat's `lease_until - observed_at` is from 25 to 30 seconds.
14. `count(distinct txid) = count(*)` on `jw.effects`. `pg_stat_database.xact_commit` from the prepare snapshot through the report is at least the effect count.
15. `jobs.leases.target_fence_certified` is not true for any row.

## Known gaps left in the read-only core

- The seven functions do not `NOTIFY`.
- There is no schedule function. Schedule and `jobs.leases` column grants remain.
- `jobs.schedules` has no payload column.
- There is no checkpoint column and no side-effect table.
- `retry_failure` has no jitter.
- Terminal completion does not return `already_applied`.
- `target_fence_certified` cannot be set. Target fencing stays in SP04.
- The applicator still has to compare file bytes. The view does not.
- The a079 `40P01` lock cycle stays possible. This driver retries a schedule fire at most three times and does not take advisory lock `6401202601`.
- `jobs.enqueue` catches the dedupe unique violation and then `SELECT`s the row. Under read committed, a concurrent inserter that has not committed is invisible, and the function raises `OO016`. A schedule fire treats that as a lost race. It does not start another tick when one occurrence has committed.
- The reviewed demo `pg-wrap.sh` sets `max_connections=20`. One hundred sessions need a higher cap. This launch keeps the other server flags and sets `max_connections=128`.

## Capture window

`capture_window` uses the same seven lease functions and the same 30 second lease. It does not add a `jobs` function. `jobs.schedules` still has no period column and no payload column. `jw.schedule_terms` holds `period_ms` and the closed payload. `jw.schedule_put`, `jw.schedule_set_period`, `jw.schedule_set_enabled`, and `jw.schedule_set_pin` are the schedule API. `jw.schedule_api` joins those terms to the published schedule row. A period change does not rewrite `next_run_at`. Disable writes `jobs.schedules.enabled` and the term in one statement. Existing capture bindings are copied into terms once, with `ON CONFLICT DO NOTHING`. The pin and artifact hash live on `jw.capture_pins`. The registration payload in `jw.schedule_payloads` is insert-only and is not the fire payload.

A due fire locks the schedule row, reads `period_ms` from `jw.schedule_terms`, refuses to enqueue when a queued or running occurrence exists, and otherwise calls `jobs.enqueue` with payload `{"v":1,"binding":"<uuid>","pin":N}`. The same transaction inserts `jobs.schedule_occurrences` and sets `next_run_at` from the database clock plus that period. A repeated tick conflicts on the occurrence key or returns `replayed`. An in-flight task payload has no period field. Payload keys `secret`, `secret_ref`, `password`, `argv`, and `shell` are rejected, as is any `secret://` text.

`capture-step` claims only `capture_window`. It reads the pinned capture file, checks the driver allowlist and the `secret_ref` file shape, and calls the published full-import functions. The legal window is Doris `str_key` and `nz_obs` snapshot rows from `oo.doris.capture-tick/v1`. `part_day`, ClickHouse, MongoDB, and StarRocks capture files are refused before `COPY`. A watermark row is written only in the same control transaction as `jobs.complete_success`. Native apply rollback leaves no receipt and does not advance the watermark. A committed target receipt whose control mirror did not commit is finished by `full_import_recover` = `replay` and does not `COPY` again. On the native path, `MirrorPending` is reached only when `verify_inputs` trust and an observed live lease are both present and `mirror_committed` is false. The observed lease is `jobs_observation.call=locked`, both certificate fields false, `inserted=false`, a non-empty owner, a uuid id, and generation at least 1. The worker passes the task id and generation already returned by `jobs.claim`. It does not call `jobs.claim` again, does not insert `jobs.leases`, and does not set `target_fence_certified`. A result with `call=claimed`, `inserted=true`, or either certificate true is refused. The worker then leaves `advanced_at` where it is and does not exec a second import.

The job payload cannot carry argv, a secret, or an artifact path. `secret_ref` on the binding is a path to a hex token. The handler does not print it. `target_fence_certified` stays false. This window does not install pipeline `0002`, `0006`, `0008`, or `0009`, and it does not execute source SQL.

A `jw.capture_sources` row pins `doris-reader` at the retained ELF digest, command `capture-tick` only, an IPv4 host, a port, and an evidence directory under `linux/run-source-*`. `secret_ref` is either a directory or `secret://oo-dl01w1-43/reader`, which resolves to `/tmp/oo-dl01w1-43-ns2-reader`. The password file is not a column and is not a job argument. `capture-pause-fe` disables every schedule whose source host and port match, through `jw.schedule_set_enabled`, in one transaction. `capture-resume-fe` enables those same rows through `jw.schedule_set_enabled(..., true)`. It does not change `period_ms` and it does not restart a source container. A disabled schedule does not exec `capture-tick`. A stored `native/{physical}/{batch}` receipt is held, and the reader still runs for the new window. An evidence directory that already contains `native-applied.json` is not re-read. The historical ELF `4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe` from pipeline commit `2e0bd098363ace48bda9b9127f5fabe6d3a1647d` stays outside `REPAIRED_LOADER_SHA256`. The allowlist is the Linux ELF `56514ae7e734ab211365c37b8889644f93a979dc82326708a7cfc3a7012e9866` from commit `e37a6f9dead618283d654c70c65302f14c430d37`. `import-native-update` runs only that file. A file this worker writes as `oo.source-admission.capture-receipt/v1` is not producer trust. The worker prints `producer_trust=not_asserted`. `producer_trust.entry=capture_receipt_file` stays refused, including when `trusted` is true. `Advance` requires `producer_trust.entry=verify_inputs`, `producer_trust.trusted=true`, `source_bound=true`, `loader_invoked=true`, `jobs_observation.call=locked`, both `target_fence_certified` fields false, `jobs_lease_receipt.inserted=false`, a non-empty owner, a uuid id, generation at least 1, and `awaiting_publish` with `mirror_committed` true. `Replay` requires that same observation, `load_state=replay`, and mirror `inserted` or `replay`. A loader or reader refusal compares `advanced_at` and leaves it unchanged. `jobs.complete_success` is the only transaction that moves the watermark. This worker does not set `target_fence_certified` and does not write a watermark state of ok. The 12bb22 `source_bound=true` line is the old loader file-compared-with-itself result. It is not trust.
