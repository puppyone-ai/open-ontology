# oo.pipeline.target-fence-receipts/v1

Version `target-fence-receipts/1`, phase `expand`, remains the historical contract. Its JSON file and the `0000` and `0001` SQL bytes stay in place. Version `target-fence-receipts/2` in `target-fence-receipts-v2.json` is the historical forward segment correction. Its JSON file and `0002` stay in place. Version `target-fence-receipts/3` in `target-fence-receipts-v3.json` supersedes that expansion when installing from v1 and upgrades a database that already has v2. Checksums are SHA-256 of the migration files. None of these contracts starts a Loader or accepts SP04-W05 or AT-PL-05.

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

## Segment contract, target-fence-receipts/2

`0001` accepts a segment when its boolean expression is true and also when that expression is unknown. A null index, a null count, or both, with a non-empty bitmap, was stored. `0002` keeps the v1 files and replaces the CHECK and the receipt function. The only stored shapes are both fields null with an empty bitmap, or both fields non-null, `count >= 1`, `0 <= index < count`, and a bitmap of 1 to 8192 bytes. The CHECK is `(expression) IS TRUE`. The function raises `23514` when the same expression `IS NOT TRUE`, and it does that before the fence check, so a stale generation is not reported as `OO013` for an unknown segment.

A database that already holds an illegal receipt aborts `0002` with `OO021` and a fixed message. The migration does not delete, update, or rewrite receipts. Lawful receipts, including `target_commit_at` and `fence_generation`, stay byte-for-byte.

`_oo.schema_ready` remains the v1 checksum comparison. A true result does not authorize a driver that requires `target-fence-receipts/2`. That driver is authorized only by the external conjunction: it hashes `0002_target_segment_null_expand.sql` itself, and `_oo.segment_contract_ready` returns true for that digest. The function does not read the file. A checksum stored by the caller is not that hash. `_oo.segment_service_readiness` is the v2 version marker and does not open the file.

## Preflight contract, target-fence-receipts/3

`0002` raises `42P01` when `_oo.apply_receipts` is absent, because its presence test folds a regclass cast before it can raise `OO002`. The reference applicator in `crates/oo-pipeline/tests/segment_preflight_correction.rs` does not execute `0002` on the way to version 3. From v1 it runs `0003_target_segment_preflight_expand.sql` only. That file keeps the closed CHECK and `commit_apply_receipt` rule from `0002`, resolves the relation with `to_regclass`, and raises `OO002` with message `target-fence-receipts/1 segment relation is absent` when the schema, the relation, or `apply_receipts_segment_check` is missing. No marker is inserted on that failure. A transaction that only rolls back `42P01` is not this result.

An existing v2 database is upgraded by the same file. The v1 and v2 rows, their checksums, and their `applied_at` values stay. `0003` does not replace `_oo.segment_contract_ready` or `_oo.segment_service_readiness`. It refuses, before any catalog change, if `commit_apply_receipt` or those v2 objects exist and are not owned by `oo_target_owner`.

`_oo.segment_preflight_ready` is the v3 comparison. A true result for a digest authorizes a version-3 driver only together with that driver's own SHA-256 of `0003`. v1 `schema_ready` and v2 `segment_contract_ready` are not substitutes. The function does not open the file. A stored lie is true only for the lie.

These functions do not fence arbitrary SQL. Another connection that knows the owner token and generation cannot commit. A role that is later granted ordinary table DML can still write user data without adopting a generation. That gap closes when the Loader driver is the only writer.

Reject events, the control-database mirror, and stream acknowledgement are not outputs of this contract.

## Backup marker

Durable tables are listed in `backup_tables`. The version marker is `_oo.schema_migrations.version = 'target-fence-receipts/1'` together with its checksum. This contract does not record a backup or a restore.
