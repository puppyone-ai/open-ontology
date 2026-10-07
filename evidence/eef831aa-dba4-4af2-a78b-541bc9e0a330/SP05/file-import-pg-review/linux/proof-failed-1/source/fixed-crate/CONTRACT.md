# file-import-pg/v1

First-release child of SP05-W05. CSV, TSV, and JSONL only. Replace only.
This file is the freeze for `tools/file-import-pg`. It is not parent acceptance,
not AT-FILE-06, and not the rest of SP05-W05.

Fixed archive: `72dc9e2db841de4de368216bbbe13d738f850ea9`.
The tool is a standalone workspace. It does not join the root workspace and
does not edit root `Cargo.toml` or `Cargo.lock`.

## Path

Reviewed `oo.source.file-worker-parent/v1` pulls verified Arrow batches.
This crate projects those batches with `assemble_batch`, then
`copy-encoder/v1` and `copy_sink` binary COPY into one LOGGED `_oo` shadow.
Receipts go through `_oo.full_import_write_batch`, `_oo.full_import_note_table`,
and `_oo.full_import_seal_run` on one connection and one transaction.
`validate-file`, `parse-file`, and `import-pg` are not this child.
`0005` is not edited.

## Caller result and stored state

A successful commit returns protocol `oo.pipeline.file-import-pg-result/v1`
with `state` `awaiting_review`, `published` false, and `role_grant` false.
The sealed row stores `awaiting_publish`. That stored state grants nothing
and publishes nothing. Admin review, ACL, and live pointer publication stay
outside this child. `run_id` is present only on the inserting call. Replay
returns `run_id` null and does not adopt a generation.
`target_generation` is present only on the inserting call.
`stored_position_codec` is `snapshot-zero/v1`.
`stored_position_lineage` is `full-import.snapshot`.
`full_import_write_batch` hardcodes those two strings and is the only writer
that advances `applied_rows`, which `note_table` requires. This child does
not pretend the receipt is `batch-cursor-us/v1`.
`parsing_mode` is `trusted_public_fixture`.
`kernel_restriction` is `unsupported`. That is not a containment certificate.

## Request

Protocol `oo.pipeline.file-import-pg/v1`. Documents are parsed with
`oo_core::parse_json_value` (duplicate keys, depth, node budget, trailing
bytes). Larger than `DOCUMENT_MAX_BYTES` (1048576) is refused. Unknown fields
are refused. Keys named `password`, `passwd`, `secret`, `authorization`,
`credential`, `private_key`, `ca_pem`, `ca_body`, or `api_key` are refused
anywhere. A key named `token` is refused except at `profile.null.token`,
which is the delimited null marker and not a credential.

```
protocol, strategy, parsing_mode, locale, type_policy,
object {sha256, path}, worker {sha256, path},
profile {format, id, bom, header, null, columns[]},
budget {batch_rows, batch_bytes, max_record_bytes, max_field_bytes,
        max_total_bytes, max_rows, max_columns, max_fields,
        worker_deadline_ms, parent_deadline_ms},
target {host, port, database, user, server_name, ca_path},
fence {shard}, mapping {namespace, table}, source
```

`bom` and `header` are required for csv and tsv and forbidden for jsonl.
`max_field_bytes` is required for every format and is passed only to the
delimited budget. JSONL null policy is `{"policy":"json"}`. Delimited null
policy is `{"policy":"empty"}` or `{"policy":"token","token":"..."}`.
Column objects always contain `name`, `parse`, `pg`, `nullable`,
`primary_key`, `precision`, `scale`. `precision` and `scale` are JSON null
except on decimal.

Closed inputs:

- strategy `replace` only. `append` and `upsert` are `unsupported_strategy`.
- parsing_mode `trusted_public_fixture` only. `production_untrusted` and
  `runsc` are `unsupported_mode`. No fake certificate is produced.
- locale `C` or `C.UTF-8` only. This is an admission check. The server locale
  is not changed.
- type_policy `preserve_row` only.
- format `csv`, `tsv`, or `jsonl` only. `parquet`, `excel`, `xlsx`, `sqlite`,
  `sql_dump`, and `dump` are `unsupported_format` and are not parsed.
- parse/pg pairs: `int64`/`int8`, `bool`/`bool`, `utf8`/`text`, `utf8`/`json`,
  `decimal`/`numeric`, and JSONL-only `json_text`/`json`.
  Decimal is delimited only. Scale is `0..=precision` and precision is `1..=38`.
  Negative numeric scale is unsupported. Typmod for numeric is
  `((precision << 16) | scale) + 4`. Other approved oids use typmod `-1`.
  Oids: bool 16, int8 20, text 25, json 114, numeric 1700.
- at least one `primary_key` column. `primary_key` true requires `nullable` false.
- names used as logical identifiers are `pgident/v1` safe names.
- source matches `^[a-z][a-z0-9_]{0,62}$`. Shard matches
  `^[a-z][a-z0-9_.:-]{0,127}$`.
- `target.host` equals `target.server_name`. TLS is `VerifyFull` against the
  CA file at `ca_path`. The password is a separate argument, never a request
  field and never an environment variable.

## Bytes

The object and the worker are absolute regular files. Symlinks are refused.
Length above `max_total_bytes` is refused before a child starts. SHA-256 is
streamed. The object hash is compared to `object.sha256` before
`ParentConfig::start`. A mismatch returns `hash_drift` with `child_started`
false and opens no database transaction. The same hash is the worker request.
After `finish`, the object is hashed again. A mismatch returns `hash_drift`
with `child_started` true and does not begin a transaction. Retry uses the
request hash. It never adopts the new bytes under the old hash.
The worker executable hash is compared to `worker.sha256` before start.

The parent must return `ParentParseComplete`. Truncated IPC, a worker error,
a malformed line, or a typed overflow fails the run. No shadow is sealed and
source progress is not advanced. JSONL presence `missing` is `missing_field`
and is not stored as SQL NULL. Delimited presence `empty` on utf8 is an empty
string. Quoted fields stay values. JSON oid 114 stores exact JSON text with
logical type Text. JSON text `null` is not SQL NULL.

An empty completed parse still copies one empty binary batch `b0` with
`applied_pos` `0` so `note_table` can see a receipt and `applied_rows` 0.
Positions are 0-based across batches. The last row of a non-empty batch stores
that row's plain position. Batch ids are `b0`, `b1`, ... and do not contain
the file hash.

## Digests

All digests are lowercase hex SHA-256. `source_generation` is the constant 1.
The fence generation is only what `adopt_target_generation` returns on the
inserting transaction.

Config preimage, file bytes and paths excluded:

```
file-import-pg/v1 config
strategy=replace
parsing_mode=trusted_public_fixture
locale=<locale>
type_policy=preserve_row
source=<source>
source_generation=1
namespace=<namespace>
table=<table>
format=<csv|tsv|jsonl>
profile_id=<id>
bom=<refuse|strip|require|none>
header=<absent|exact|none>
null=<empty|json|token:<token>>
column=<name> parse=<parse> pg=<pg> nullable=<true|false> primary_key=<true|false> precision=<n|-> scale=<n|->
```

Type preimage:

```
file-import-pg/v1 type
type_policy=preserve_row
column=<name> oid=<oid> typmod=<typmod> nullable=<true|false>
```

Schema preimage:

```
file-import-pg/v1 schema
namespace=<namespace>
table=<table>
column=<name> oid=<oid> typmod=<typmod> not_null=<true|false> primary_key=<true|false>
```

Canonical preimage, one lowercase hex row hash per line:

```
file-import-pg/v1 canonical
count=<rows>
<row canonical hash>
```

Payload preimage. This is not the full-import quality-byte continuation.
`copy_sha256` is the SHA-256 of the binary COPY header, row chunks, and trailer.

```
file-import-pg/v1 payload
object_sha256=<64 hex>
copy_sha256=<64 hex>
rows=<rows>
batch_index=<index>
```

Hook digest is SHA-256 of the exact bytes `file-import-pg/v1 hook none`
with no trailing newline. `input_digest` is the object SHA-256 hex.
Physical names come from `plan_physical_mapping` at `run_generation` 1.
`register_physical_mapping` returns `inserted` or `replay`, not the name.

## Transaction

Recover every batch with `_oo.full_import_recover` before `BEGIN`.
All `replay` returns `awaiting_review` and does not adopt. A mix of `replay`
and `absent` is `incomplete_receipt` and writes nothing. `OO010` is
`hash_conflict` and writes nothing. Any other refusal rolls back.

On an insert, one transaction does: `BEGIN`, `adopt_target_generation`,
`full_import_bind_session`, register namespace then table then columns,
`full_import_open_run`, `full_import_prepare_shadow` at table index 0,
COPY then `full_import_write_batch` for each batch, `full_import_note_table`,
`full_import_seal_run`, `COMMIT`. Shadow relpersistence is `p`.
`prepare_shadow` revokes PUBLIC and grants INSERT to `oo_pipeline_runtime`.
This child grants no role and does not publish a live pointer.

`stage_replace` stops after `write_batch` and before `note_table`, holding
the open transaction and `pg_backend_pid`. Dropping that connection closes
the socket. PostgreSQL aborts the transaction. A backend terminate at that
point leaves no shadow, no run, and no receipt. A fresh replace of the same
bytes copies once. This does not claim the signal arrived mid-packet.

Copy budgets are fixed: value 1048576, row 4194304, chunk 8388608,
rows per chunk 1024. Unavailable cells are refused.

## Errors

Error protocol `oo.pipeline.file-import-pg-error/v1`. Fields are `protocol`,
`code`, `detail`, `child_started`, `published`. `detail` is a static token.
SQLSTATE is mapped and the server message is dropped: `OO010` hash_conflict,
`OO014` writer_gate, `OO018` fence_session, `OO022` mapping_conflict,
`OO024` mapping_absent, `OO030` coverage, `OO031` run_closed,
`23514` and `22023` closed_input, otherwise `database`.

CLI exits: 0 `awaiting_review`, 2 refusal, 3 `hash_conflict`, 4 worker
failure, 1 any other error. Arguments are the request path and the password
file path. The password value is not an argument.

## Not claimed

Append, upsert, Parquet, Excel, sqlite, SQL dumps, production untrusted
parsing, runsc, and an escaped sandbox are out of scope. Direct-child reap
is observed by process absence after `finish` or `Drop`. `setsid` descendants
are outside that observation.
