# session-release-v1

Bounded local-fixture orchestrator for the accepted durable adapter
`session-store-v1` and the pure reducer `session-transition-v1`.
This crate drives `record_release_step` and `cancel`. It does not
reimplement the reducer. It never sends `complete_release`.

`physical_release_certified`, `auth_provider_used`, and `runtime_used` stay
false. `production_sandbox` stays false. `containment` is `local_fixture`.
Exit 0 means the five fixture steps are confirmed and the session is still
`releasing`. That is not a certified release and not a closed session.

## Supported fixture adapters

| Adapter | Step | What it does |
|---|---|---|
| `direct_child_harness` | S1 | Spawn the operator argv as a direct child in its own process group. `SIGTERM` the group, wait up to 10s, then `SIGKILL` and a short reap. Confirm only when the direct child has exited, `/proc` was readable, and every snapshotted descendant pid and starttime is gone. |
| `fixture_pg_backend_cancel` | S2 | On a dedicated `oo_session_cancel` connection, read the registered pid from `pg_stat_activity`, require the query role, application name, and database, reject superuser and denylisted roles, then `pg_cancel_backend`. The boolean is recorded and is not success. Success is a later observation that the same backend is exactly `idle`, or that the pid is gone and no active backend still has that application name. `idle in transaction` is not stopped. A dropped socket is not stopped. |
| `fixture_bounded_collect` | S3 outputs, S4 transcript | After the child has stopped, read at most 65536 bytes. A larger file, a missing file, or a symlink is not confirmed, and the workspace is not destroyed. |
| `fixture_directory_destroy` | S8 | Recursively delete one directory the operator registered, without following symlinks, then require `symlink_metadata` to be `NotFound`. The evidence adapter is `local_fixture_directory`. |

The operator supplies the identity, project, session, lease revision, backend pid, application name, query role, harness argv, and the two UUID-owned directories. The store checks compare-and-swap. A restarted process loads the durable revision. It does not adopt the previous child: a live pid is `harness_not_owned` and is not signalled; a missing pid is `unproven_absence` and does not confirm S1.

## Explicitly unsupported

S5 snapshot encryption, S6 session token, S7 proxy mapping, S9 warmup pool, S10 permits, S11 close, authentication, token revoke, network, runsc, sandbox, and the 30s reaper daemon. This process is one invocation, not a daemon. Those actions stay `unsupported` on the receipt and unset in the reducer. S5 is not skipped. Filling S6–S11 as confirmed is not a supported way to close.

## Order and clocks

Paths are classified before any store call. A foreign path is not deleted. A missing workspace is not absence. The checkpoint directory is UUID-owned and is not inside the workspace.

`cancel` is persisted, and the phase is `releasing`, before the harness is spawned or the backend is cancelled. The checkpoint fsync records the wall-clock epoch. Deadlines use monotonic elapsed in the first process and that persisted epoch after a restart. A backward wall clock fails closed. Exactly 5000, 10000, and 60000 milliseconds are inside the budget. The next millisecond is `slo_miss`.

The 5s admission and backend branch does not wait for the 10s harness wait. S2 is persisted when the backend observation finishes. Collection and destroy run only after the direct child has exited with no escaped descendant, and only inside the 60s resource deadline.

Reducer `now_ms` is the operator clock, then `last_now_ms + 1` for later events. It is not the deadline clock.

## Roles

The runtime login must be `oo_session_runtime`. The cancel login must be `oo_session_cancel`. This crate does not issue grants or other admin DDL. The fixture installer, which is not this binary, creates the roles and grants `oo_fixture_query` to `oo_session_cancel`. Query roles `postgres`, `oo_fixture_admin`, `oo_session_admin`, `oo_session_runtime`, and `oo_session_cancel` are refused before connect. A `rolsuper` query role is refused after connect.

## Exit

| Code | When |
|---|---|
| 0 | S1, S2, S3, S4, and S8 are confirmed in this process and in the reducer, S8 absence was verified, S5–S7 and S9–S11 are absent, phase is `releasing`, and no deadline was missed. |
| 1 | Malformed request. |
| 2 | Incomplete work, foreign or missing path, revision or event-id conflict, escaped descendant, or deadline miss. The phase is not forced to `closed`. |
| 3 | PostgreSQL connect, TLS, or `commit_unknown`. No success is invented. |

Passwords are accepted on stdin JSON only. They are cleared before this process returns. They are not arguments, environment variables, or receipt fields.

## Pins

Standalone workspace. Path dependencies are only `../session-store` and `../session-transition`. Registry pins are the session-store lock: serde 1.0.229, serde_json 1.0.151, tokio 1.53.1, tokio-postgres 0.7.18. SHA-256 is the in-crate FIPS 180-4 function. No new registry crate. a207 remains the only writer of the connector manifest and the root lock.
