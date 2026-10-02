# oo.pipeline.target-fence-receipts/v1

Version `target-fence-receipts/1`, phase `expand`. The SQL lives in `crates/oo-pipeline/migrations`. Checksums in `target-fence-receipts-v1.json` are SHA-256 of those files. This contract does not start a Loader and does not accept SP04-W05 or AT-PL-05.

## Inputs

- Target database. The fixture name is `oo_agent`. Bootstrap uses `current_database()`.
- Installer role that can create `NOLOGIN` roles and revoke `pg_catalog` execution. The fixture installer is a local superuser and is not a product runtime role.
- `0000_bootstrap_roles.sql` as that installer, then `0001_target_fence_expand.sql` as `oo_target_owner`.
- Applicator checksum variables, 64 lowercase hex characters, for `target-bootstrap/1` and `target-fence-receipts/1`.

The control database is a different database. This contract does not write it. The fixture creates an empty `oo_control` only to show that a receipt transaction does not cross databases.

## Interfaces

| Object | Interface |
|---|---|
| `oo_target_owner` | `NOLOGIN` schema owner. Runs the expand file. Not granted to the runtime. |
| `oo_pipeline_runtime` | `NOLOGIN` pipeline principal. Execute on the six public functions. Select on bookkeeping and `service_readiness`. |
| `_oo.target_generations` | Shard generation, owner token, session nonce, immutable lock keys, `adopted` or `failed`. |
| `_oo.apply_receipts` | Receipt identity `(source, table_generation, txn_or_batch_id)` plus digests, position, completion, and database commit time. |
| `_oo.adopt_target_generation` | Session advisory lock. Renews in place or allocates the next generation. |
| `_oo.renew_target_generation` | Refreshes `renewed_at` for the generation this backend holds. |
| `_oo.fail_target_generation` | Marks the generation failed and releases the session lock. |
| `_oo.assert_target_fence` | Proves this backend holds the lock in the current transaction. |
| `_oo.commit_apply_receipt` | Inserts one receipt or returns `replay`. Same identity and a different payload raises `OO010`. |
| `_oo.schema_ready` | True only when the expand checksum matches. A mismatch returns false. |
| `_oo.service_readiness` | `security_invoker` version marker. It does not open the SQL file. |
| `_oo.migration_lock` | Singleton row. Advisory key `6404202605` is the migration executor lock, not a shard fence. |

`lock-key/v1` is MD5 of `target-fence-lock/v1|` plus the shard id, split into two int4 keys. The caller does not pass the keys. Clock authority is `clock_timestamp()`.

## Outputs

A migrated database has both bookkeeping rows, the singleton lock row, and the grants in the expand file. Ready means `schema_version_present` is true and the stored checksum equals the release file. A checksum mismatch, a missing row, or a rolled-back migration is not ready.

An identical receipt retry returns `replay` and does not change the stored row or its commit time. A different payload, codec, or lineage leaves the original row in place. Fence generation is stored and is not part of that equality.

## What a caller still has to do

`commit_apply_receipt` does not write user data or quality rows. The Loader has to do that in the same target transaction. The fixture schema `fixture_synth` is the demonstration, and it is not part of this migration. A receipt can exist without those rows if the caller only invokes the receipt function.

These functions do not fence arbitrary SQL. Another connection that knows the owner token and generation cannot commit. A role that is later granted ordinary table DML can still write user data without adopting a generation. That gap closes when the Loader driver is the only writer.

Reject events, the control-database mirror, and stream acknowledgement are not outputs of this contract.

## Backup marker

Durable tables are listed in `backup_tables`. The version marker is `_oo.schema_migrations.version = 'target-fence-receipts/1'` together with its checksum. This contract does not record a backup or a restore.
