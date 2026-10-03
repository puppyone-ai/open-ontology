# physical-mapping/v1

Version `physical-mapping/v1`, phase `expand`, file `crates/oo-pipeline/migrations/0004_physical_mapping.sql`. Migration `0003_target_segment_preflight_expand.sql` already occupies `0003`, so this binding is `0004`. It leaves `0000`, `0001`, `0002`, and `0003` byte-for-byte in place.

Logical names are complete `pgident/v1` strings. A safe name is lowercase `[a-z_][a-z0-9_]*`, at most 63 bytes, not reserved, and not already starting with `q_`. Every other original is `q_` plus the full lowercase hex of its Unicode 17 NFC UTF-8 bytes. This mapping stores that finished string. It does not compose, case-fold, apply NFKC, or truncate. A `q_` form whose byte length is 63 is closed. The maximum logical name is 4096 bytes, with no NUL.

`pgident/v1` already gives `é` and `e` plus U+0301 one string, `q_c3a9`. `ﬁ` (U+FB01) stays `q_efac81` and `fi` stays `fi`. `Customer` stays `q_437573746f6d6572` and `customer` stays `customer`. Those pairs remain distinct identities here.

## Scope

One row binds:

- source token `^[a-z][a-z0-9_]{0,62}$`
- config digest, 64 lowercase hex characters
- run generation, a bigint at least 1
- level `namespace`, `table`, or `column` (stored as `n`, `t`, or `c`)
- logical namespace, and the logical table and column required by that level

The stored PostgreSQL identifier is 63 bytes: `pn_`, `pt_`, or `pc_`, then the first 60 hex characters of the SHA-256 of the canonical payload (240 bits). `UNIQUE (identity_digest)` and `UNIQUE (physical_name)` reject a collision. The caller does not choose the physical name. The SQL function recomputes the digest and refuses a different 32-byte value with `OO022`. A missing digest or a digest whose length is not 32 is `23514`.

Canonical payload, with a single `0x00` after each listed field and no `0x00` after the level byte:

`physical-mapping/v1`, source, config digest, decimal generation with no leading zeros, level byte `n`/`t`/`c`, logical namespace, logical table, logical column.

An empty table or column is the empty string at the levels that do not use it. A table registration requires the namespace row in the same source, config, and generation. A column registration requires that namespace row and the table row. A missing parent is `OO024`. A parent whose stored physical name differs from the recomputed name is `OO022`.

## Interfaces

| Object | Interface |
|---|---|
| `plan_physical_mapping` | Pure Rust plan. Returns the digest, the 63-byte name, and the parent physical names. No connection. |
| `quote_physical_name` | Double-quotes a generated `pn_`/`pt_`/`pc_` name. Any other text is refused. |
| `REGISTER_SQL` / `LOOKUP_SQL` | `_oo.register_physical_mapping($1..$8)` and `_oo.lookup_physical_mapping($1..$7)`. Parameters only. |
| `_oo.physical_mappings` | Logged table, owner `oo_target_owner`. No grant to `PUBLIC` or `oo_pipeline_runtime`. |
| `_oo.register_physical_mapping` | `SECURITY DEFINER`. Identical row returns `replay` and leaves `registered_at`. New row returns `inserted`. |
| `_oo.lookup_physical_mapping` | `SECURITY DEFINER` read of the stored name. It does not hash. A missing row is `OO024`. |

The register call takes `pg_advisory_xact_lock` on `hashtextextended` of the identity before it reads the row. If that key equals the migration lock `6404202605`, it uses the next integer. The caller owns the transaction. Rollback leaves the identity absent. A concurrent duplicate becomes `replay` or a masked `OO022`.

Direct `INSERT` without `_oo.physical_mapping_writer = 'register'` is `OO014`. `UPDATE`, `DELETE`, and `TRUNCATE` are `OO014` even when that setting is present. The setting is transaction-local and cleared before the function returns. This gate is `_oo.physical_mapping_writer`. It does not set `_oo.writer` and it does not open the receipt fence.

`oo_pipeline_runtime` may execute register and lookup. It cannot read the table or the hash helper. `PUBLIC` cannot execute either function. An agent role that is not a member of the runtime cannot read the mapping.

## Install

Run `0000_bootstrap_roles.sql` as the installer, then `0001_target_fence_expand.sql` as `oo_target_owner`, then this file as `oo_target_owner`. Record `schema_migrations` version `physical-mapping/v1`, phase `expand`, filename `0004_physical_mapping.sql`, checksum SHA-256 of the file bytes. The same checksum on a later install leaves `applied_at` in place. A different checksum is `OO001` and does not change the row.

The live fixture database is created from `template0` with encoding `UTF8`, `LOCALE_PROVIDER builtin`, and `BUILTIN_LOCALE 'C.UTF-8'`.

## Boundary

This primitive stores identifier bindings. It does not create user tables, copy user rows, write shadows, or insert apply receipts. It is not the Loader and it does not admit the reader or target apply.
