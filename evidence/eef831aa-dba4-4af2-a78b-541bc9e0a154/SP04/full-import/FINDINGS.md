# Findings

## The fence could not see backend_start until the owner inherited pg_read_all_stats

`0000` creates `oo_target_owner` `NOINHERIT` and was not rewritten. A plain `GRANT pg_read_all_stats` stored `inherit_option=false`. Security-definer functions owned by that role then read `backend_start` as null and raised `OO018` ("target backend identity is not bound"). That is attempt 3, and attempt 4 after the grant was compiled without `INHERIT TRUE`.

`install_full_import` now runs `GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE` on the install path before `SET LOCAL ROLE`, and again on the replay path because replay does not re-execute `0005`. `0005` itself requires `pg_has_role(..., 'usage')` and applies the same grant only when the current user is still superuser. The reason is narrow: the fence's existing `pg_stat_activity` read has to see the caller's `backend_start`. `SET FALSE` blocks `SET ROLE` to the stats role. Runtime and Agent are not members. Pid, owner, generation, and the held shard lock are still checked. The grant is documented in `0005`, `migrations/README.md`, `contracts/pipeline/full-import-v1.md`, and `full-import-v1.json`. It is not a security proof.

## The generation check rejected a fresh replay owner

Attempt 7 reached rollback, crash-before, and conflict, then panicked with `OO013` "target generation or owner is stale". Replay had adopted a new random owner while `replay_batch` binds the owner stored on the ambiguous commit. The test now adopts that stored owner. The new backend still receives a new target generation. The `OO013` check was not removed. The green run reports `crash_after=replay`.

## Deadline retirement happened at open_stream

Attempt 6 slept past the deadline. `open_stream` returned the deadline error on the first pull, and the run stayed `open` because that error did not retire it. Cancel had already retired. `ensure_stream` now retires the run when `open_stream` fails. The green run reports `deadline=retired` and `cancel=retired`.

## The coherent-cut writer needed the audit table

Attempt 5 passed the fence cases and then panicked: permission denied for `app.write_audit`. The source trigger is security invoker. The fixture writer inserts into `app.orders` and must also be able to insert the audit row. The reader role is not granted that insert. Source reader writes stayed 0 on the green run.

## Clippy ran only after the component was added in the same container

The smoke downloaded clippy with `HOME=/tmp`, which does not survive the next container. Attempt 8 passed both live tests and exited 1 because `cargo-clippy` was not installed for `1.94.0-aarch64-unknown-linux-gnu`. The verify script now runs `rustup component add clippy --toolchain 1.94.0` before clippy. Attempt 9 added the component (exit 0) and then clippy denied nine lib lints: explicit auto-deref, needless borrows of empty `Vec`s, a large `Held::Open` variant, and a manual multiple-of check. Those were fixed in `full_import.rs`. The green clippy, including tests, exited 0.

## Vendor warnings stayed in the dependency

`sqlx-postgres` printed two unfulfilled `dead_code` expectations. `cargo clippy -p oo-pipeline --all-targets --locked --offline -- -D warnings` still exited 0. Those vendor lines were not edited.

## What the green oracle does and does not cover

The public entry on one PostgreSQL 17.10 compared two selected tables, counts, values, and canonical digests, and exercised same-shard refusal, other-shard import, takeover, atomic rollback, crash before commit, crash-after replay, conflicting identity, a writer during the read followed by a fresh generation, bounded backpressure, an oversized batch that retires the run, and an explicit `jsonb` refusal. Agent stage and quality reads and the shadow insert were denied. The installer live test still runs the frozen routes in one invocation and the `0005` install, replay, checksum refusal, and runtime refusal in the same test, as a second `cargo` invocation.

`jsonb` refusal is not a certification of every PostgreSQL type. Control mirror, job, and acknowledgement remain later work and were not written. The mapping-only route that runs `0004` alone was already uncovered by the a150 live fixtures and was not added here.
