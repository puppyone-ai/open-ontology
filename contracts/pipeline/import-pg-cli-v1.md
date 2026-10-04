# import-pg command

`oo-pipeline import-pg` reads one `oo.pipeline.import-pg/v1` document from stdin and writes one `oo.pipeline.import-pg-result/v1` document to stdout. The command takes no further arguments. It does not install a schema, take bootstrap credentials, or run a migration. The caller installs the reviewed target schema with the existing installer API before invoking the command. Production import verifies `0005` readiness on the runtime connection and then calls `import_unpublished`. A missing or inexact `full-import/v1` row fails before a source session is opened, and the command does not install it.

`RunSummary` and `RunState` have public fields. A Rust caller can therefore build an in-memory `awaiting_publish` value. This command does not. Stdout is the `RunSummary` returned by `import_unpublished`. The `_oo.full_import_run` row is the seal. `awaiting_publish` is not publication. If stdout cannot be written after that return, the process exits 1 and the stored row stays.

Exit 0 is success. Exit 2 is usage: the command word is missing, it is not `import-pg`, or another argument is present. Usage does not start the runtime and does not read stdin. Exit 1 is invalid input, a source failure, a target failure, an import failure, or I/O. Failure writes no result document and does not report `awaiting_publish`. Diagnostics are static. They name a field or a source fact. They do not include a password, a CA body, a path, a driver message, a server message, or a URL that contains a password.

## Request

The document is at most 1_048_576 bytes. `oo_core::parse_json_value` rejects duplicate keys, excess depth, excess nodes, and trailing bytes. `serde` then rejects unknown fields and JSON null in an optional field. A present budget, stream, copy, or type-policy object requires every field of that object. An omitted object uses the default below.

`protocol` is `oo.pipeline.import-pg/v1`. Any other string is an invalid version.

`source.source_id` matches `^[a-z][a-z0-9_]{0,62}$`. `source.host` is a DNS name of at most 253 characters, without a scheme, a colon, or a slash. `source.port` is 1 through 65535. `source.database` and `source.username` are 1 to 63 characters of ASCII letters, digits, and underscore. `source.secret_ref` is `secret://` plus 1 to 200 characters of ASCII letters, digits, and `.`, `_`, `/`, `-`. `source.password_file` and `source.ca_file` are paths of regular files. The password file is at most 1026 bytes. One trailing LF, or one trailing CRLF, is removed. The remaining password is 1 to 1024 bytes, contains no NUL, and is not written to the output. The CA file is at most 1 MiB and must be a certificate bundle. A private key is rejected. `source.tables` is a non-empty list of `{schema, table}` catalog names, each 1 to 63 bytes and without NUL. Duplicate pairs are rejected.

`source.budget`, when omitted, is connect 30000 ms, statement 60000 ms, lock 30000 ms, `max_rows` 10000, and `max_bytes` 8388608. Each timeout is 1 to 120000 ms. `max_rows` is 1 to 100000. `max_bytes` is 1 to 33554432.

`target.host`, `target.port`, and `target.database` use the same rules as the source endpoint. `target.runtime_principal` is the login name and must equal both `CURRENT_USER` and `SESSION_USER` after connect. `target.password_file` and `target.ca_file` use the same file rules as the source. TLS for both sides is verify-full with the caller CA and an empty root store. The command does not disable TLS and does not use native or webpki roots.

`shard` matches `^[a-z][a-z0-9_.:-]{0,127}$`. `owner` is a UUID in 8-4-4-4-12 hexadecimal with hyphens. `source_generation` is 1 through 9223372036854775807. `deadline_ms`, when omitted, is 60000. The accepted range is 1 to 120000. The deadline is checked by snapshot preparation and by the existing owner. It does not cancel a statement that has already been sent.

`stream`, when omitted, is `batch_rows` 1024, `batch_bytes` 1048576, `max_value_bytes` 65536, and `max_row_bytes` 262144. The accepted bounds are `batch_rows` 1 to 65536, `batch_bytes` 1 to 33554432, `max_value_bytes` 1 to 1048576, and `max_row_bytes` 2 to 4194304 and at least `max_value_bytes`.

`copy`, when omitted, uses those same value and row caps, `max_chunk_bytes` 1048576, and `max_rows_per_chunk` 1024. The accepted bounds are `max_value_bytes` 1 to 1048576, `max_row_bytes` at least the value cap and at most 4194304, `max_chunk_bytes` 19 to 8388608 and at least `max_row_bytes`, and `max_rows_per_chunk` 1 to 65536.

`type_policy`, when omitted or present, is `unsupported` `preserve_row` and `reject` `preserve_row`. No other policy is accepted. The request has no SQL field.

## Preparation

`PgConnector::prepare_snapshot` opens the configured read-only session, reads `inspect_access`, and refuses `registration_allowed = false` before reading the catalog. It then reads the catalog and the existing snapshot-node statement (`pg_is_in_recovery`, `pg_postmaster_start_time`, `current_database`, `SESSION_USER`, `CURRENT_USER`). The access-report principal and recovery flag must be known and equal to that row. The postmaster text must be non-empty, at most 128 bytes, and free of control characters. Admitted columns and primary-key names come from that catalog. `config_identity` and `type_policy_identity` are computed from the configuration and the policy. The preparation session is closed before the function returns. `SnapshotOwner::open` still rechecks the node, the catalog, and the connection cap.

## Result

Stdout is one canonical JSON object, keys sorted, with a trailing newline. The fields are `protocol` (`oo.pipeline.import-pg-result/v1`), `run_id`, `state` (`awaiting_publish`), `source_generation`, `target_generation`, `table_count`, and `tables`. Each table has `schema`, `name`, `physical_name`, `rows`, `canonical_digest`, and `columns`. Each column has `name`, `attnum`, `physical_name`, and `type_oid`. Table physical names match `^pt_[0-9a-f]{60}$`. Column physical names match `^pc_[0-9a-f]{60}$`. The digest is 64 lowercase hexadecimal characters. These values are the fields of the returned summary, in selection order.
