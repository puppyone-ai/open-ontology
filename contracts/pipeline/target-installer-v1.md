# target-installer/v1

`oo_pipeline::target_installer` installs the frozen target schema on one caller-owned PostgreSQL connection. Protocol `target-installer/v1`. It is the production installer for `0000_bootstrap_roles.sql`, `0001_target_fence_expand.sql`, `0003_target_segment_preflight_expand.sql`, and `0004_physical_mapping.sql`. It is not the Loader. It does not copy user rows, write apply receipts, create a publication, or admit a source reader.

The SQL files stay byte-for-byte. This installer does not invent a migration runner and it does not expose an arbitrary SQL adapter.

## Connection

`install` and `verify_readiness` take `&mut sqlx::PgConnection`. The caller opens that connection. This crate does not build a pool, read a password, or choose a host.

The connection must be idle. `install` opens one transaction. Bootstrap requires a superuser, because `0000` creates roles and revokes `pg_catalog` execution. An existing catalog requires a superuser or a member of `oo_target_owner`. `oo_pipeline_runtime` is not that administrator.

`verify_readiness` is the runtime path. It does not bootstrap, `SET ROLE`, `ALTER ROLE`, take `pg_advisory_xact_lock(6404202605)`, or execute a migration script.

## Scripts and digests

The four scripts are compiled with `include_str`. `compiled_digests` is the lowercase hex SHA-256 of those bytes. The caller does not pass a hash.

| Version | File | Role during install |
|---|---|---|
| `target-bootstrap/1` | `0000_bootstrap_roles.sql` | Administrator, fresh database only |
| `target-fence-receipts/1` | `0001_target_fence_expand.sql` | `oo_target_owner`, fresh database only |
| `target-fence-receipts/3` | `0003_target_segment_preflight_expand.sql` | `oo_target_owner`, after v1 exists or during fresh install |
| `physical-mapping/v1` | `0004_physical_mapping.sql` | `oo_target_owner`, after `0003` returns |

`0002_target_segment_null_expand.sql` is not compiled into this installer and is not executed. `HISTORICAL_V2_SHA256` is the frozen digest of that file. A stored `target-fence-receipts/2` row must match it. The installer does not insert that version.

## Transaction

`install` takes `pg_advisory_xact_lock(6404202605)`, then `SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE` when that row exists. The key is the migration executor lock, not a shard fence.

Catalog reads use `to_regclass` and a constraint lookup. They do not cast `_oo.apply_receipts` to `regclass`.

| Observed database | Scripts | Markers written |
|---|---|---|
| No `_oo` schema, no migration relations, and neither product role | `0000`, `0001`, `0003`, `0004` | bootstrap, v1, v3, `physical-mapping/v1`, in that same transaction |
| Valid bootstrap and v1, no v2 row, relation and constraint present, v3 and mapping absent | `0003`, `0004` | v3 and mapping. No v2 row |
| Valid bootstrap, v1, and v2, relation and constraint present, v3 and mapping absent | `0003`, `0004` | v3 and mapping. v1 and v2 checksums and `applied_at` stay |
| Valid bootstrap, v1, v3, and mapping, and v2 absent or equal to the frozen v2 digest | none | none. `applied_at` stays |
| Valid v3, mapping absent | `0004` | mapping |
| Stored checksum for bootstrap, v1, v2, v3, or mapping differs | none | none. PostgreSQL raises `OO001`, message `migration checksum mismatch` |
| Not a fresh catalog, and `_oo.apply_receipts` or `apply_receipts_segment_check` is absent | `0003` | none. `0003` raises `OO002`, message `target-fence-receipts/1 segment relation is absent` |

Roles are cluster-wide. A database without `_oo` whose cluster already has `oo_target_owner` is not the fresh row. `0000` is not edited to skip `CREATE ROLE`.

A statement failure rolls the transaction back. Objects and markers from that attempt are absent. `verify_readiness` does not return success.

The installer does not update or delete `_oo.schema_migrations`. It does not delete or rewrite `_oo.apply_receipts`. A receipt that violates the closed segment rule makes `0003` raise `OO021` and roll back. That is not a repair.

## Readiness

`verify_readiness` returns `VerifiedReadiness` only when the four production rows exist, their phase is `expand`, their filenames are the files above, and their checksums equal `compiled_digests`. When a v2 row is also present it must match `HISTORICAL_V2_SHA256`, phase `expand`, and `0002_target_segment_null_expand.sql`. The value exposes those compiled digests. It does not accept a caller hash.

`_oo.schema_ready`, `_oo.segment_contract_ready`, and `_oo.segment_preflight_ready` still compare an argument the caller supplies. They are not this readiness check. A stored checksum that was not computed from these files does not make `VerifiedReadiness`.

Missing migration table is `not_installed`. A partial production set with matching checksums is `incomplete`. A differing checksum is `checksum_mismatch` (`OO001`).

## Boundary

Schema install is not apply. No user table is created. No `COPY` is issued. No publication is created. No receipt is inserted. Source registration is not enabled. A runtime role that can verify readiness cannot install and cannot assume `oo_target_owner`.
