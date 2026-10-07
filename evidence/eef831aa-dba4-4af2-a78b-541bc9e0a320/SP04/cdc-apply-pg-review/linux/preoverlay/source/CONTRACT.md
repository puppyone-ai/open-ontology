# cdc-apply-pg v1

Fenced writer for one recovered normalized PostgreSQL CDC transaction.
Contract name: `cdc-apply-pg-v1`.

This crate is the atomic target writer under CDC-D04. It reads one sealed
`cdc-spool-v1` directory and writes the selected rows through the installed
target fence. It does not read Debezium, a replication slot, or Valkey.
It does not ACK, XACK, TRIM, or write a control-database mirror.
It does not install schema, register mappings, or grant privileges.

`source_cross_table_consistent`, `upstream_commit_lsn_verified`,
`debezium_normalizer`, `p2_verified`, `p11_verified`, and `ack_allowed`
stay false. `commit_position_provenance` stays `caller_declared`.
A stored receipt may set the database column `cross_table_consistent`
only when this process applied every selected event for the bound tables
in one target transaction and the receipt call returned `inserted` or
`replay` for those same bindings. That flag is not a source or WAL
certificate.

The parent SP04 loader, segmented apply, and stream acknowledgement are
not this crate. A transaction of more than 200000 events, or a request
that names a segment, returns `needs_segments` before any target
connection is opened.

Pinned direct crates are already in the repository lock. `cargo update`
is not part of this contract. Path dependencies are `oo-pipeline`,
`oo-core`, and `cdc-spool` at the fixed source tree. SHA-256 for binding
digests is `oo_pipeline::target_installer::sha256_hex`.

## Request

UTF-8 JSON file. Unknown fields are refused. The password is read from
`password_file`. It is not an argument.

```json
{"contract":"cdc-apply-pg-v1","spool_dir":"/spool","endpoint":{"host":"oo-a301-pg","port":5432,"database":"oo_a301","user":"oo_a301_runtime","password_file":"/secrets/runtime.pass","ca_file":"/secrets/ca.crt","server_name":"oo-a301-pg"},"shard":"billing","owner":"11111111-1111-4111-8111-111111111111","generation":1,"table_generation":"g1","run_generation":1,"lineage":"<64 lowercase hex>","source":"billing","epoch":1,"control_lease_valid":false,"segment":null,"keys":[{"table":"public.orders","ordinal":0,"name":"id","pg_type":"int8"}],"columns":[{"table":"public.orders","ordinal":0,"name":"qty","pg_type":"int8","nullable":true}]}
```

`host` and `server_name` must be the same DNS name. TLS is rustls with
the caller CA only, and the server certificate must match `server_name`.
`control_lease_valid` is required and ignored. A lease is not a fence.

`segment` must be JSON null. An object is `needs_segments`.
`shard` matches `^[a-z][a-z0-9_.:-]{0,127}$`.
`table_generation` matches `^[a-z][a-z0-9_]{0,62}$`.
`owner` is a lowercase UUID. `generation` and `run_generation` are
integers `>= 1`. `epoch` must fit in PostgreSQL `integer`.

`keys` and `columns` use logical table names `schema.table` from the
spool bind. Key ordinals are key-cell ordinals. Column ordinals are
payload ordinals. Names are `pgident/v1` logical names. Declared
`pg_type` is only `bool`, `int2`, `int4`, `int8`, or `text`.
Any other type is `unsupported_type` before a target connection.
A key is non-null. Each selected table needs at least one key.

## Digests

Hex strings are lowercase SHA-256. The spool bind hashes must equal
these documents. Each line ends with LF.

Config, from the request:

```text
cdc-apply-config/v1
source=<source>
epoch=<epoch>
lineage=<lineage>
shard=<shard>
table_generation=<table_generation>
run_generation=<run_generation>
position_codec=pg-commit-lsn/v1
provenance=caller_declared
```

Schema, tables sorted, then keys and columns by ordinal:

```text
cdc-apply-schema/v1
table <schema.table>
key <ordinal> <name> <pg_type>
col <ordinal> <name> <pg_type> <0 or 1>
```

Type policy, then the same table lines:

```text
cdc-apply-type-policy/v1
zero_date=null
tinyint1=smallint
strip_nul=true
```

The three policy lines are the `oo_core` default `TypePolicy`: null zero
dates, `tinyint1` as smallint, and NUL stripped only when a caller later
uses that policy. This writer refuses a text cell that contains NUL
instead of storing the stripped text.

Hooks, from the spool `hook_hashes` in stored order:

```text
cdc-apply-hooks/v1
<hook hash>
```

Payload, every recovered event in `global_txn_sequence` order, selected
or not:

```text
cdc-apply-payload/v1
<event hash>
```

The receipt stores those five digests. `position_codec` is
`pg-commit-lsn/v1`. `position_lineage` is the spool lineage. The
installed function accepts that lineage only when it matches
`^[a-z][a-z0-9_.:-]{0,127}$`, so a 64-hex lineage has to start with
`a` through `f`. Any other lineage is `identity` before a connection.
`applied_pos` is the spool decimal position. `source_epoch` is the spool
epoch. The batch id is the spool transaction id and must match
`^[A-Za-z0-9_.:-]{1,128}$`.

## Values

Cells stay distinct. `null` is a known SQL null. `unavailable` is not
written as null.

Conversion uses `oo_core::convert::convert_cell` and the default type
policy. Integers use signed `MysqlIntKind::Big`. `int8` accepts the
closed range `-9223372036854775808` through `9223372036854775807`.
A value outside the declared width is `out_of_range`. A cell whose type
is not the declared type is `type_mismatch`. A rejected conversion is
`conversion_rejected`. These faults happen before a target connection.
A null written into a non-null column is `null_required`. A payload that
does not list each declared column once is `payload_incomplete`.

An upsert lists every declared payload column once. On insert, any
unavailable payload cell is `unavailable_insert` and the transaction
rolls back. On update, an unavailable column is omitted, so the stored
value stays. A delete removes by the exact key only. Zero deleted rows
is success. The key is required and cannot be null or unavailable.

## Target transaction

One dedicated `tokio-postgres` connection. No pool.

Before that connection, recovery must be a sealed spool whose manifest
attestations are still false. `manifest.count > 200000` or a non-null
`segment` returns `needs_segments` and does not open the connection.

On the connection, in order:

1. Read `_oo.schema_migrations` and compare it with
   `oo_pipeline::target_installer::compiled_digests`. The required rows
   are `target-bootstrap/1`, `target-fence-receipts/1`,
   `target-fence-receipts/3`, and `physical-mapping/v1`, all phase
   `expand`. A historical `target-fence-receipts/2` row, when present,
   must match `HISTORICAL_V2_SHA256`. Full-import is not required.
2. Require `adopt_target_generation`, `assert_target_fence`,
   `commit_apply_receipt`, and `lookup_physical_mapping` to be owned by
   `oo_target_owner`.
3. `SELECT` `oo_pipeline::physical_mapping::LOOKUP_SQL` for the namespace,
   the table, and each column. The stored name must equal
   `plan_physical_mapping`. A missing row is `mapping_absent`. A
   different name is `mapping_foreign`.
4. `SELECT _oo.adopt_target_generation(shard, owner)` on this connection.
   Another backend holding the lock is `fence_held`. No user row and no
   receipt is written. The returned generation is the generation this
   connection holds. The request generation is not sent to the receipt
   function when it differs.
5. In one SQL transaction: `assert_target_fence`, then
   `_oo.commit_apply_receipt` with an empty segment bitmap and both
   segment fields null, `partial_txn` false, `txn_complete` true, and
   `cross_table_consistent` true. Then the selected row writes, in
   global sequence order. Then `COMMIT`.

`replay` writes no rows. `inserted` writes the rows in that same
transaction. `OO010` is `payload_conflict`; the stored receipt stays.
A statement failure, including `CDC_APPLY_INJECT=row_failure`, rolls
the transaction back. The receipt and the row writes disappear together.

The database column `cross_table_consistent` is true only on that
committed receipt. The JSON field `source_cross_table_consistent` is
false on every output.

## Commit outcome

`COMMIT` uses the simple query `COMMIT`. When the response is an I/O
error, or `CDC_APPLY_INJECT=hide_commit` hides it, a new connection
adopts and calls `_oo.commit_apply_receipt` again.

- `replay` means the receipt is durable. The result is `applied` and
  no row is written.
- `inserted` means the first commit did not persist. The rows are
  written on this second connection and committed.
- Any other reconcile error, including a refused second connection, is
  `commit_unknown`. `target_applied` is false. Exit 3.

`CDC_APPLY_RECONCILE=down` forces that second connection to fail.
`CDC_APPLY_CHECKPOINT=before_commit` or `after_commit_before_response`
writes `CDC_APPLY_MARK` and waits. Those variables are unset in
ordinary runs.

## Output

One JSON object, one line.

Applied exit 0:

```json
{"contract":"cdc-apply-pg-v1","kind":"applied","fault":null,"outcome":"inserted","target_applied":true,"ack_allowed":false,"db_cross_table_consistent":true,"source_cross_table_consistent":false,"upstream_commit_lsn_verified":false,"debezium_normalizer":false,"p2_verified":false,"p11_verified":false,"control_lease_consulted":false,"commit_position_provenance":"caller_declared"}
```

`outcome` is `inserted` or `replay`. Refusals use `"kind":"refused"`,
`target_applied` false, and `db_cross_table_consistent` false.
Exit 2 is a refusal. Exit 3 is only `commit_unknown`. Exit 1 is usage
or I/O while reading the request.

## CLI

`cdc-apply-pg apply <request.json>`

No other arguments. The process does not take a superuser, does not
`GRANT`, and does not call `pg_advisory_lock` itself.

## Out of scope

Segmented 500000-event apply, source WAL cross-table proof, Debezium,
P2, P11, control mirror, ACK, XACK, TRIM, and publication. a299 has
not accepted the spool this crate reads. This crate does not accept
SP04 or the parent pipeline.
