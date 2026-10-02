# oo-pipeline target fence and receipts, target-fence-receipts/1

Expand-phase migration for the Agent database fence. This is the target-side foundation of SP04-W05. It is not the Loader, not CDC, and not AT-PL-05.

The durable tables are `_oo.target_generations` and `_oo.apply_receipts`. Both are `LOGGED`. Receipt identity is `(source, table_generation, txn_or_batch_id)`. The row stores `config/v1`, `type-policy/v1`, `batch/v1`, five lowercase SHA-256 digests, a position codec and lineage, a non-negative scale-0 applied position, completion flags, an optional segment bitmap, the fence shard and generation, and `target_commit_at`. `target_commit_at` is `clock_timestamp()` inside the receipt function. The caller does not pass it.

Reject-event durability stays in the control database. This schema has no reject-event table and no control-database mirror. Stream acknowledgement is not written here. A control-database lease is not a row in `_oo.target_generations` and is not read by these functions.

## Files

| Version | Phase | File | What it does |
|---|---|---|---|
| `target-bootstrap/1` | expand | `0000_bootstrap_roles.sql` | Creates `oo_target_owner` and `oo_pipeline_runtime`. Revokes `pg_advisory%` and `pg_try_advisory%` execution, plus `set_config(text, text, boolean)`, from `PUBLIC`. |
| `target-fence-receipts/1` | expand | `0001_target_fence_expand.sql` | Creates `_oo`, the fence tables, and the receipt functions. |
| `target-fence-receipts/2` | expand | `0002_target_segment_null_expand.sql` | Replaces the segment CHECK and `commit_apply_receipt` so unknown index/count combinations are rejected. Does not rewrite receipt rows. |

Checksums are SHA-256 of those file bytes. `target-fence-receipts/1` is recorded in `contracts/pipeline/target-fence-receipts/target-fence-receipts-v1.json`. `target-fence-receipts/2` is recorded in `target-fence-receipts-v2.json` and repeats the v1 file hashes so a reader can see they did not change. The checksum is not inside the SQL file. A later edit of any of these files is a new version, not a silent rewrite.

There is no migrate-phase or contract-phase file in this slice.

## Applicator

The Rust driver is not in this crate. Until it exists, `tests/pg_fixture.sh` is the reference applicator. It passes fixed `psql -f` paths and hex checksum variables. It does not concatenate SQL text.

The applicator takes `pg_advisory_xact_lock(6404202605)` and then `_oo.migration_lock` `FOR UPDATE`. That key is only the migration executor lock. It is not a shard fence. The jobs control lock `6401202601` is a different database and a different key.

On an empty target database the applicator runs both files in one transaction. It then inserts the two `_oo.schema_migrations` rows in a second transaction. A run with the same checksums locks, compares, and does not execute the files again. A missing bookkeeping row raises `OO002`. A different checksum raises `OO001` and does not change the row. A failed file rolls back with its transaction. `_oo.service_readiness.schema_version_present` is true only when the `target-fence-receipts/1` expand row exists. `_oo.schema_ready(checksum)` compares the stored checksum and returns false on a mismatch. Neither one opens the file.

`_oo.schema_migrations` rejects update and delete.

`0002_target_segment_null_expand.sql` runs as `oo_target_owner` after `target-fence-receipts/1`. It locks `_oo.apply_receipts`, refuses with `OO021` when a stored receipt is not one of the two closed segment shapes, and does not delete or update receipt rows. The applicator inserts the `target-fence-receipts/2` row only after that file returns, in the same transaction. A repeat with the same checksum does not execute the file again. A different checksum raises `OO001` and leaves the row. A failure rolls back the function, the CHECK, and the readiness row together.

`_oo.schema_ready` still means `target-fence-receipts/1` only. It does not authorize a driver that requires the closed segment contract. That driver computes the SHA-256 of `0002_target_segment_null_expand.sql` itself and calls `_oo.segment_contract_ready` with that digest. The function compares the argument to the stored row. It does not open the file. `_oo.segment_service_readiness.schema_version_present` is true when the v2 expand row exists and does not prove the file bytes.

## Roles

`oo_target_owner` is `NOLOGIN`, `NOSUPERUSER`, `NOCREATEDB`, `NOCREATEROLE`, `NOINHERIT`, `NOBYPASSRLS`, and `NOREPLICATION`. It owns `_oo`. The installer creates it. The migration does not grant that role to the runtime.

`oo_pipeline_runtime` has the same login and privilege limits. It is the internal pipeline principal. It is not an Agent credential and it is not a Gateway credential. Its `search_path` is `pg_catalog`.

The bootstrap script is the trusted installer. It has to run as a role that can create roles and revoke `pg_catalog` grants. The schema owner is not that installer. The expand file runs as `oo_target_owner`.

`oo_pipeline_runtime` can execute `adopt_target_generation`, `renew_target_generation`, `fail_target_generation`, `assert_target_fence`, `commit_apply_receipt`, and `schema_ready`. It can select `_oo.schema_migrations` and `_oo.service_readiness`. It cannot select or modify the generation or receipt tables. `PUBLIC` has no execute grant on these functions. Internal helpers, including the session-nonce and write-gate functions, are not granted.

The fixture creates real `LOGIN` members after bootstrap. Those roles are not part of the product migration. `oo_a072_runtime` inherits `oo_pipeline_runtime` and cannot `SET ROLE` to it. `oo_a072_bootstrap` can `SET ROLE oo_target_owner` and does not inherit that role. Gateway and Agent logins are not members and are not granted `CONNECT`.

## What the database enforces

Shard ids, lineages, and source names are closed tokens. Digests are 64 lowercase hex characters. Unknown codecs and lineages raise `OO015`. A too-long identity raises `23514` and is not truncated into a PostgreSQL name. `table_generation` is not a PostgreSQL identifier. `pgident/v1` `q_` hex names can exceed 63 bytes, and PostgreSQL would silently truncate them. This migration does not build DDL from logical names. Fixture tables use fixed short names: `fixture_synth.applied_row` and `fixture_synth.quality_row`.

Lock keys are `lock-key/v1`: MD5 of `target-fence-lock/v1|` plus the shard id, split into two int4 advisory keys. The caller does not supply the keys. The first adoption stores them. Changing the shard id or either key raises `OO019`. A cross-shard collision raises `OO020` and releases a lock acquired by that call.

`adopt_target_generation` takes `pg_try_advisory_lock` on this backend. The same backend, the same owner token, and the same session nonce renew in place and do not increment the generation or stack the lock. Any other call from a backend that already holds the lock raises `OO017`. A backend that newly acquires the lock inserts generation 1 or increments the stored generation. Generation does not decrease and does not wrap past `bigint` max (`22003`).

The session proof is a nonce in `pg_temp.oo_target_fence_session`, created by the owner inside the adopt function. The runtime role has no `TEMP` privilege. Another connection cannot read that temp table. `assert_target_fence` and `commit_apply_receipt` lock the generation row `FOR UPDATE` in the same call that checks `pg_locks` for this backend's exclusive advisory lock. A committed assert does not authorize a later transaction. The receipt function checks the fence again before it writes.

A failed generation rejects later receipt calls with `OO018`. `fail_target_generation` releases the session lock. That unlock survives a rollback of the fail transaction. After a rolled-back fail the row is still `adopted` and the lock is gone, so a later receipt raises `OO011`. After a committed fail, a new adopt allocates the next generation. A stale owner or a stale generation raises `OO013` when this backend holds the lock. A connection that only knows the owner token and the generation raises `OO011`.

The same receipt identity with the same digests, codec, lineage, position, completion flags, and segment bitmap returns `replay` and does not change `target_commit_at` or insert another row. Fence generation is recorded and is not part of that equality, so a later owner can retry the same batch. A different payload, codec, or lineage raises `OO010` and leaves the stored row in place. Applied positions are not compared across codecs or lineages. There is no `max(applied_pos)` API.

Direct insert, update, delete, and truncate raise `OO014` when the transaction-local writer gate is unset. Receipt updates and deletes raise `OO014` even when the gate is set. A generation decrease raises `22003`. That generation comparison sits inside the `target_generations` branch, so an `apply_receipts` insert does not resolve `NEW.generation`. A `DELETE` that matches no row does not fire the row trigger. The fixture deletes an existing generation and checks that the row remains. The runtime role has no table DML grant, so its direct writes fail with `42501` before the trigger. The owner role is caught by the trigger.

`SECURITY DEFINER` is limited to the fence and receipt functions. Each one sets `search_path` to `pg_catalog` or `pg_catalog, _oo`. They are not `LEAKPROOF`. No extension is installed. The functions do not take dynamic SQL from the caller.

## What this does not enforce

`commit_apply_receipt` writes a receipt and nothing else. A later Loader has to issue user-data and quality DML in the same target transaction, then call this function. The fixture schema `fixture_synth` demonstrates that bundle and is not part of the product migration. Calling the receipt function alone can store a receipt without those rows.

These functions do not intercept arbitrary SQL. A role that is later granted table DML can write user data without adopting a generation. Fencing that path is the Loader driver's job. This slice does not claim that integration.

The advisory lock is held by one session. A pool connection that did not call `adopt_target_generation` cannot reuse another session's owner token. Losing the session releases the lock. The generation row remains until a new backend adopts and increments it.

No publication is created. No `oo_r_*` read role is granted. The control database is a separate fixture database and is not updated in the receipt transaction. Control mirror and stream acknowledgement remain pending. Source registration is not enabled here.

## Backup

Backup consumers need `_oo.schema_migrations`, `_oo.migration_lock`, `_oo.target_generations`, and `_oo.apply_receipts`. The version marker is `target-fence-receipts/1` and its checksum. Views and functions are rebuilt by the migration. This repository does not claim a backup or a restore was run.

## Tests

`tests/target_fence_contract.rs` checks the v1 file text and the published v1 checksums. `tests/segment_null_correction.rs` checks the v2 checksums and, when run against one owned PostgreSQL 17 fixture, the segment cases. `tests/pg_fixture.sh` remains the v1 applicator. None of these is the 100-worker acceptance test, a dual-architecture matrix, a Loader, CDC, or a production deploy.
