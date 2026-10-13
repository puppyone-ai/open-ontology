# local-pg-mirror/v1

Child SRC-D01 / E43. This file is the freeze for `tools/local-pg-mirror`
after the 2026-10-08 20:07:59 source-copy plan. It is not parent acceptance,
not a customer copy, and not a release.

Fixed dependency archive: `54e8f8ea1def6ef351e8f6a0957c242315283c63`.
The tool is a standalone workspace. It does not join the root workspace and
does not edit root `Cargo.toml`, root `Cargo.lock`, or dependency crates.

`customer_complete` is always false. A `mirror_state` of `complete` means the
selected public or explicitly configured snapshot was stored on the local
target. It does not mean an enterprise database was copied or tested.

`SOURCE_COPY_BINARY` is `local-pg-mirror`.
`SOURCE_COPY_CONFIG_PROTOCOL` is `oo.pipeline.local-pg-mirror/v1`.
`sealed_import` returns the import generation, import hash, plan hash,
manifest hash, target-config hash, and snapshot id only when `mirror_state`
is `complete` and `consistency` is `single_snapshot`.

## Engines

`engine` must be `postgresql`. Any other value, including `mysql`, returns
`unsupported_engine` before a socket, before a credential read, and before a
budget check. There is no second interpreter for an unknown engine.

## One source connection

Discover still calls `PgConnector::prepare_snapshot` once
(`oo.source.pgsql/v1`). That call reads access, the catalog, and the node,
then drops its connection before it returns. Copy does not call
`prepare_snapshot` again and does not call `SnapshotOwner::open`.

The copy holds one source backend. Catalog, row pages, and the health read
share that backend's `REPEATABLE READ` `READ ONLY` transaction. The manifest
`source_socket_peak` is the maximum of
`count(*)` from `pg_stat_activity` for this database, this user, and
`application_name = oo-pg-source`, sampled on that same transaction. It is
not a stored constant. A count above 1 aborts the cut. `recheck_connections`
is 0. `source_socket_fact` is `repeatable_read_read_only` only after
`transaction_isolation` and `transaction_read_only` have been read back
and equal `repeatable read` and `on`. A health row that is present but
does not match stores `not_verified`.
A deadline that expires before that connect returns `Ok` records peak 0,
`not_opened`, and `incomplete` / `deadline`.

There is no second catalog-guard connection. The catalog guarantee is the
same snapshot: the admitted catalog hashed at discover must equal the catalog
read inside this transaction before any target row is written. A difference
is `catalog_drift`. DDL committed after the snapshot is a different version
and is not mixed into this cut. Locks are the `AccessShare` locks of ordinary
`SELECT`, bounded by `lock_timeout`. The source receives no trial write, no
DDL, no `EXPLAIN`, no `EXPLAIN ANALYZE`, no unqualified `COUNT(*)`, and no
CDC publication. `source_count_star`, `source_explain`, and `source_ddl` stay
false. Privilege facts come from `has_table_privilege`. `SELECT` must be
true. An insert privilege bit may be recorded. It is not executed.

`pg_stat_database` tuple counters are read on this transaction at the start
and at the end. `source_write_tuples` is that delta. A non-zero delta is
`source_write` and does not seal. The tool does not call `pg_stat_reset`.

Session setup uses verify-full, no startup `options` string, statement
logging off, and `application_name = oo-pg-source`. Session `set_config`
sets `statement_timeout` and `lock_timeout` from the plan.
`idle_in_transaction_session_timeout` is the snapshot deadline, so the open
transaction cannot outlive that budget while target batches run.
`default_transaction_read_only` is on. `TimeZone` and `DateStyle` are not
changed. The driver `begin()` is followed by `SET TRANSACTION ISOLATION LEVEL
REPEATABLE READ, READ ONLY` as the first statement. The tool does not send
`START TRANSACTION`.

Identity uses `session_user`, `current_user`, `current_database()`,
`pg_is_in_recovery()`, and `pg_postmaster_start_time()::text`. A difference
from the plan is `identity_drift`. `pg_export_snapshot()` and
`pg_backend_pid()` are recorded. Close drops this backend. The source
database is not dropped.

## Rows

Pages are keyset reads of one `int2`, `int4`, or `int8` primary key, at most
`batch_rows` (1..=1000), inside the one transaction. The first page is
`ORDER BY` the key `LIMIT`. The next page is `WHERE key >` the last key,
then the same order and limit. SQL identifiers must match
`^[A-Za-z_][A-Za-z0-9_]*$` and be at most 63 bytes, then they are quoted.
Anything else is `ident`. No primary key is `no_key`. Any other key is
`key_type`.

Result values are PostgreSQL binary. Text output is not a decoder. The local
codec matches the approved scalar decoder for oids 16, 17, 20, 21, 23, 25,
114, 700, 701, 1042, 1043, 1082, 1083, 1114, 1184, 1700, and 2950. `int8`
stays an exact integer. `numeric` is the binary digit header, rescaled to the
column precision and scale. `json` oid 114 keeps the original text after a
JSON scan. `jsonb` oid 3802 is `unsupported_column` before a cell is decoded,
even when a type map would call it lossless. Unavailable cells are not
written as NULL. `max_rows` is per table. `max_bytes` is per batch payload.

Bytes are `copy-encoder/v1` through one `COPY ... FORMAT binary` of the
encoded buffer. The payload digest is SHA-256 of those COPY bytes followed by
the `full-import/v1` quality bytes. A second encoder pass must produce the
same bytes before COPY starts. The table digest is SHA-256 of the
concatenation of the per-row 32-byte canonical hashes from `assemble_batch`.
Target shadows are the installed LOGGED `full-import/v1` tables. Receipts are
`_oo.full_import_stage_decision` and `_oo.full_import_write_batch`. An empty
table still writes one empty batch before `_oo.full_import_note_table`.

Names are `physical-mapping/v1` (`pt_` / `pc_` / `pn_` plus 60 hex
characters). The manifest keeps the original schema, table, and column names.
`_oo_pos` is the loader `numeric(38,0)` position, not a source column.
`schema_digest` is the admitted catalog hash. It is not a second schema hash.
Config, type, and input digests are the public snapshot identities.
`hook_digest()` is the public full-import hook.

After every selected batch, and before seal, the target admin checks shadow
`count(*)`, `count(DISTINCT key)`, and `pg_attribute.atttypid` /
`atttypmod`. It then creates a non-concurrent primary-key btree named `ix_`
plus a prefix of the physical table name, and runs
`analyze_mapped_shadows` in chunks of at most 8. Seal
(`_oo.full_import_seal_run`) runs only when every selected table is
exhausted. Grant of `USAGE` on `_oo` and `SELECT` on this run's shadows
happens only after seal. A grant failure leaves `mirror_state` `incomplete`.
Partial shadows stay ungranted.

## Budgets

Closed limits. A looser value is `budget` before connect.

- `connect_timeout_ms` 1..=30000
- `statement_timeout_ms` 1..=30000
- `lock_timeout_ms` 1..=1000
- `batch_rows` 1..=1000
- `snapshot_deadline_ms` 1..=300000
- `max_rows` 1..=100000
- `max_bytes` 1..=33554432

The snapshot deadline starts when the copy attempts the source connection,
after target install. It is not extended for install or for a later chunk.
Expiry discards this unvalidated shadow. The tool does not open another
snapshot to finish the cut.

## Failure, spool, and close

`mirror_state` is `complete`, `incomplete`, or `refused`.
`consistency` is `single_snapshot` only when `mirror_state` is `complete`.
Every other outcome is `not_a_snapshot`.

A source disconnect, timeout, catalog drift, or identity drift discards the
partial spool and retires an unsealed run. A durable spool is written beside
the manifest and is bound to the plan hash, generation, and snapshot id. It
stores COPY bytes and a sidecar of batch id, row count, payload digest,
canonical hashes, and applied position. The spool is marked complete only
after the whole source cut is read. Resume checks the payload digest again
and refuses a tampered spool. Resume of a complete spool does not open the
source and does not read the source secret. Resume of an incomplete manifest
without a complete spool is `resume_refused` and does not connect and does
not create a missing secret path. A complete manifest is returned without a
connection and without another write. Snapshot ids are not mixed.

`CopyStop::CancelBeforePull` opens the source, opens a target run, retires
it before any batch, and closes the source. `StopAfterBatches` does not
leave a complete spool. `DropAfterSnapshot` finishes the spool, does not
leave an unsealed run, and returns `incomplete` / `source_dropped` with
`spool_state` `complete`.

Close does not drop the source database or a persisted target copy. The
source secret is not written into the manifest or the target config. The
target file protocol remains `oo.pipeline.local-pg-mirror-target/v1`: host,
port, database, reader user, server name, CA SHA-256, and `tls` `verify-full`.
It has no password and no source field. `read_local` opens only that target.
Display text uses the closed oid list. It also reads `pg_attribute` for the
shadow columns. Any other oid is `unsupported_read`.

## Files

Input config, plan, manifest, and target connection config are JSON parsed
with `oo_core::parse_json_value`. Unknown fields are refused. Keys named
`password`, `passwd`, `secret`, `authorization`, `credential`, `private_key`,
`ca_pem`, `ca_body`, `api_key`, or `token` are refused. `secret_ref` is
allowed and is not a password.

Credential files are absolute regular files. Symlinks are refused. The plan
does not contain a credential path or a password. The plan stores the CA
SHA-256. A CA change is `ca_drift` before connect. An existing plan file is
not rewritten (`plan_exists`).

The plan hash is SHA-256 of the canonical preimage below. A mismatch is
`plan_tamper` and does not connect.

```
local-pg-mirror/v1 plan
engine=postgresql
source_id=<id>
host=<host>
port=<port>
database=<database>
user=<user>
server_name=<server>
ca_sha256=<64 hex>
secret_ref=<secret://...>
table=<schema>.<table>
connect_timeout_ms=<n>
statement_timeout_ms=<n>
lock_timeout_ms=<n>
max_rows=<n>
max_bytes=<n>
batch_rows=<n>
snapshot_deadline_ms=<n>
generation=<n>
shard=<shard>
owner=<uuid>
target_host=<host>
target_port=<port>
target_database=<database>
target_admin_user=<user>
target_runtime_user=<user>
target_reader_user=<user>
target_server_name=<server>
target_ca_sha256=<64 hex>
admitted_sha256=<64 hex>
expected_principal=<user>
expected_database=<database>
expected_in_recovery=<true|false>
expected_postmaster_sha256=<64 hex>
```

Admitted preimage is the compact JSON object `tables`, each with `schema`,
`name`, `columns` (`attnum`, `name`, `oid`, `typmod`, `not_null`), and
`primary_key`, in selection order.

The import hash preimage, hashed only for a sealed run, is:

```
local-pg-mirror/v1 sealed-import
plan=<plan_sha256>
generation=<import_generation>
snapshot=<snapshot_id>
rows=<rows>
table=<schema>.<name> rows=<n> digest=<64 hex>
```

`host` equals `server_name` on both ends. TLS is verify-full with the caller
CA only. Passwords are not environment variables and not connection options.
The source password is zeroized before the copy returns.

## Fixture proof

The proof uses two private PostgreSQL 17.10 instances, source and target,
each with its own data directory, role passwords, and server certificate.
The generic card allows one private server. This copy card asks for two
independent instances. The proof records that deviation and does not reuse
another worker's server. A monitoring connection used by the test is an
admin session and is not the source login.

The public fixture has the five-row gold tables plus `public.batch_a` (2000),
`public.batch_b` (8000), and `public.batch_c` (90000). Numeric precision 38,
int8, null, time, and bytea use the codec above. Original names remain on the
manifest. After seal, revoking source connect still leaves `read_local` and
`pg_attribute` able to read the same local rows. `local_only` is not that
proof. Cancel, a one-batch stop, a 1 ms deadline, and an added column are
not `complete`. Replay of a sealed manifest does not connect.
