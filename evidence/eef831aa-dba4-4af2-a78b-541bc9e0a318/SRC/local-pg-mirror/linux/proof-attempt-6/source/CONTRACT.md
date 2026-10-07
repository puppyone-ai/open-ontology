# local-pg-mirror/v1

Child SRC-D01 / E43. This file is the freeze for `tools/local-pg-mirror`.
It is not parent acceptance, not a customer copy, and not a release.

Fixed dependency archive: `54e8f8ea1def6ef351e8f6a0957c242315283c63`.
The tool is a standalone workspace. It does not join the root workspace and
does not edit root `Cargo.toml`, root `Cargo.lock`, or dependency crates.

`customer_complete` is always false. A `mirror_state` of `complete` means the
selected public or explicitly configured snapshot was stored on the local
target. It does not mean an enterprise database was copied or tested.

## Engines

`engine` must be `postgresql`. Any other value, including `mysql`, returns
`unsupported_engine` before a socket, before a credential read, and before a
budget check. There is no second interpreter for an unknown engine. A
dedicated connector is a later child.

PostgreSQL uses the existing source API:

- discover and the copy recheck call `PgConnector::prepare_snapshot`
  (`oo.source.pgsql/v1`). That call uses one connection for access inspection,
  the read-only catalog transaction, and the node read, then drops it. It does
  not open `SnapshotOwner` and does not reserve a generation.
- the row copy calls `oo_pipeline::full_import::ImportSession`, which calls
  `SnapshotOwner::open` (`oo.source.pgsql.snapshot/v1`). That verified reader
  opens one repeatable-read data session and one read-committed catalog guard.
  This child does not open a third source socket and does not replace that
  reader with its own SQL. The manifest reports `source_socket_peak` 2 and
  `source_socket_fact` `verified_reader_guard` only after that open is reached.
  The recheck connection is already closed. A request cannot ask this child
  to hide the guard.

Bytes are `copy-encoder/v1` and `copy_sink` through `full-import/v1`.
Names are `physical-mapping/v1` (`pt_` / `pc_` / `pn_` plus 60 hex characters).
The local database schema is `target-installer/v1` plus `full-import/v1`.
Logical names in the manifest are the original schema, table, and column.
`_oo_pos` is the loader's `numeric(38,0)` system position, not a source column.
Type policy is fixed `PgTypePolicy::PRESERVE_ROW`. It is not a caller field
and it is not added to the plan preimage.

Discover calls `prepare_snapshot` with a deadline of the configured statement
timeout. `snapshot_deadline_ms` is stored in the plan and the clock starts
only when the copy recheck starts.

## Budgets

Closed limits. A looser value is `budget` before connect.

- `connect_timeout_ms` 1..=30000
- `statement_timeout_ms` 1..=30000
- `lock_timeout_ms` 1..=1000
- `batch_rows` 1..=1000
- `snapshot_deadline_ms` 1..=300000
- `max_rows` 1..=100000
- `max_bytes` 1..=33554432

The snapshot deadline starts when the copy recheck starts. It is not extended
when install, a slow batch, or a later chunk needs more time. Expiry stops the
copy and sets `mirror_state` `incomplete`. This child does not open another
generation to finish the cut and does not call that a consistent dump.
`mirror_state` `complete` requires `ImportSession::finish` and
`RunComplete` for the same config identity, generation, and admitted catalog.
`SnapshotFinished` is not completion.

No source DDL, DML, `EXPLAIN`, `EXPLAIN ANALYZE`, unqualified `COUNT(*)`,
lock-changing statement, or CDC publication is sent. Catalog `max_rows` is the
existing inspection cap, not a table-data count. The manifest records the
configured caps and `source_count_star` false.

Selection is an explicit list. There is no include-all wildcard. Empty is
`config`. A cut that cannot finish inside the deadline stays `incomplete`.

## Files

Input config, plan, manifest, and target connection config are JSON parsed
with `oo_core::parse_json_value` (duplicate keys, depth, trailing bytes).
Unknown fields are refused. Keys named `password`, `passwd`, `secret`,
`authorization`, `credential`, `private_key`, `ca_pem`, `ca_body`, `api_key`,
or `token` are refused. `secret_ref` is allowed and is not a password.

Credential files are absolute regular files passed as separate runtime
arguments. Symlinks are refused. The plan does not contain a credential path
or a password. CA PEM bytes are not stored. The plan stores the CA SHA-256.
A CA change is `ca_drift` before connect. An existing plan file is not
rewritten (`plan_exists`).

The plan hash is SHA-256 of the canonical preimage below. Copy recomputes it.
A mismatch is `plan_tamper` and does not connect. Resume of an incomplete
manifest for that hash is `resume_refused` and does not connect and does not
write. A complete manifest for that hash is returned without a source
connection and without another write. A different source id is a different
physical mapping. A failed or replayed run must not be treated as a write to
another source's physical tables.

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
`primary_key`, in selection order. Its SHA-256 is `admitted_sha256`.
The copy recheck must produce the same admitted bytes, principal, database,
recovery flag, and postmaster text. A difference is `catalog_drift` or
`identity_drift` before `SnapshotOwner::open`.

`host` equals `server_name` on both ends. TLS is verify-full with the caller
CA only. Passwords are not environment variables and not connection options.

## Target

The target is a separate database. `install` then `install_full_import` run
as the configured admin login before any source recheck. The runtime login
must already exist, must not be a superuser, and receives membership of
`oo_pipeline_runtime` when that membership is absent. The reader login is the
same kind of check. After `RunComplete` only, the admin grants `USAGE` on
`_oo` and `SELECT` on the shadow tables named by this run. Other shadows are
not granted. Grant failure leaves `mirror_state` `incomplete` even if the
loader sealed.

The target connection file is protocol
`oo.pipeline.local-pg-mirror-target/v1`. It has host, port, database, reader
user, server name, CA SHA-256, and `tls` `verify-full`. It has no password
and no source field. `read_local` opens only that target. It casts the closed
oid set to exact text, bool text `true`/`false`, or bytea hex. Any other oid
is `unsupported_read` and is not stringified. `DateStyle` on that session is
`ISO, YMD`.

## Rows and refusal

Null stays SQL NULL. Unavailable and masked cells stay the encoder refusal.
They are not written as NULL and are not dropped. Approved loader oids are
the `full-import/v1` set, including int8, numeric, timestamp, bytea, and json.
`jsonb` and other unapproved types are `unsupported_column`. The manifest
quarantine lists schema, table, column, oid, and the static reason. It does
not list cell bytes. No text substitute is stored.

`mirror_state` is `complete`, `incomplete`, or `refused`.
`consistency` is `single_snapshot` only when `mirror_state` is `complete`.
Every other outcome is `not_a_snapshot`.

## Fixture proof

The development proof is one private PostgreSQL 17.10 container, two
databases, and fresh public passwords in private files. Tests do not contain
a remote host or a password. Source `INSERT` as the read login is SQLSTATE
`42501`. Three tables are compared to hand-written values after the source
login can no longer connect. Cancel, a one-batch stop, and a 1 ms deadline
are not `complete`. An added column is `catalog_drift`. A failed second
source does not change the first source's local rows. Replay of the failed
plan does not connect and does not rewrite the first source.
