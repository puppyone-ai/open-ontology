# analyze-pg command

`analyze-pg` reads one `oo.pipeline.analyze-pg/v1` document from stdin and writes one `oo.pipeline.analyze-pg-result/v1` document to stdout. The command takes no arguments. An operator runs it after `init-pg` and `import-pg`, on the target database only. It calls `oo_pipeline::target_statistics::analyze_mapped_shadows`. It does not import, does not open a source database, does not publish a table, and does not grant a column.

Exit 0 is success. The other exits are the table below. Failure writes no result document. Stderr is one static line, `analyze-pg: ` plus the fact in the table. That line does not include a password, a CA body, a private key, a path, a host, a driver message, a server message, a SQLSTATE, a row value, or a sample.

| Exit | Stderr fact | When |
|---|---|---|
| 2 | `usage` | Any argument is present. Stdin is not read. |
| 3 | `invalid input` | The document, a field, the endpoint, the operator name, or `connect_ms` is rejected before a file is opened and before a connection is attempted. |
| 3 | `invalid version` | `protocol` is not `oo.pipeline.analyze-pg/v1`. |
| 3 | `identity` | `project` is not `source`, or `plan_physical_mapping` rejects the source, the digest, or the generation. Checked before either file is opened. |
| 3 | `bound` | The table list is empty, longer than 8, duplicated, or a logical name or timeout is outside the closed range. Checked before either file is opened. |
| 3 | `target password file` | The password file is missing, not a regular file, larger than 1026 bytes, not UTF-8, empty after one trailing newline is removed, longer than 1024 bytes, or contains NUL. |
| 3 | `target ca file` | The CA file is missing, not a regular file, larger than 1 MiB, or rejected by `PgTlsConfig::from_ca_pem`. A private key is rejected. |
| 4 | `target connect` | The endpoint and both files were accepted, and the verify-full connection did not open. |
| 4 | `target principal` | `CURRENT_USER` or `SESSION_USER` is not `operator`. |
| 5 | `runtime caller` | `analyze_mapped_shadows` returned `RuntimeCaller`. |
| 5 | `read-only caller` | `analyze_mapped_shadows` returned `ReadOnlyCaller`. |
| 5 | `target role` | `analyze_mapped_shadows` returned `RoleDenied`. |
| 6 | `open transaction` | `analyze_mapped_shadows` returned `OpenTransaction`. |
| 6 | `stale generation` | `analyze_mapped_shadows` returned `StaleGeneration`. |
| 6 | `mapping absent` | `analyze_mapped_shadows` returned `MissingTable`. |
| 6 | `relation absent` | `analyze_mapped_shadows` returned `MissingRelation`. |
| 6 | `relation forged` | `analyze_mapped_shadows` returned `ForgedRelation`. |
| 7 | `lock timeout` | `analyze_mapped_shadows` returned `LockTimeout`. |
| 7 | `statement timeout` | `analyze_mapped_shadows` returned `StatementTimeout`. |
| 7 | `estimate unknown` | `analyze_mapped_shadows` returned `EstimateUnknown`. |
| 7 | `estimate unusable` | `analyze_mapped_shadows` returned `EstimateUnusable`. |
| 1 | `target database` | PostgreSQL failed in some other way. The driver text is dropped. |
| 1 | `target statistics` | The library receipt is not protocol `target-statistics/v1`, or `published` or `estimate_is_exact` is true. |
| 1 | `analyze input` | Stdin could not be read. |
| 1 | `analyze output` | The maintenance transaction already committed and stdout could not be written. |

The process owns the connection and exits. The receipt field `disposition` is the library value at return (`reusable` or `fresh_connection`). This command does not keep the connection. A failure does not print the disposition. An operator transaction cannot be passed in: the command dials a new verify-full session. `OpenTransaction` is the library refusal if that session is already inside a transaction. This proof does not open one.

## Request

The document is at most 1_048_576 bytes. `oo_core::parse_json_value` rejects duplicate keys, excess depth, excess nodes, and trailing bytes. `serde` rejects unknown fields and JSON null in numeric fields. There is no password field, no SQL field, and no CA body. A field named `password` is `invalid input`. Its value is not written to stderr.

`protocol` is `oo.pipeline.analyze-pg/v1`.

`project` and `source` are the mapping identity. They must be equal. `config_digest` is 64 lowercase hex characters. `source_generation` is an integer. A value below 1 is `identity`. These rules are the public `plan_physical_mapping` checks, applied before either file is opened.

`tables` is 1 through 8 objects, each `{namespace, name}`. Names are logical. A physical name is not accepted. Duplicates, an empty list, a ninth table, and a name `plan_physical_mapping` rejects are `bound`. `statement_timeout_ms` and `lock_timeout_ms` are integers from 1 through 30000. Anything else is `bound`.

`target.host`, `target.port`, and `target.database` are checked with `PgEndpoint::new`. `target.operator` is the login. The placeholder passed to `PgCredentials` while checking that name is not sent to PostgreSQL. `target.password_file` and `target.ca_file` are paths of regular files. Symlinks are rejected. The password file is at most 1026 bytes. One trailing LF, or one trailing CRLF, is removed. The remaining password is 1 to 1024 bytes and is not written to the output. The CA file is at most 1 MiB and must satisfy `PgTlsConfig::from_ca_pem`.

`connect_ms`, when omitted, is 30000. The accepted range is 1 to 120000. It bounds the connection attempt and the principal query. It is checked before either file is opened. The maintenance transaction then uses `statement_timeout_ms` and `lock_timeout_ms`.

`PGPASSWORD`, `DATABASE_URL`, and `PGSSLROOTCERT` are not read. The connection is `PgConnectOptions::from_explicit`, `PgSslMode::VerifyFull`, and `ssl_root_cert_from_pem` of the caller CA. Statement logging is disabled.

## Result

Stdout is one canonical JSON object, keys sorted, with a trailing newline. `published` and `estimate_is_exact` are copied from the library receipt and must both be false. The command does not rewrite a true value into false. The other fields are the receipt:

| Field | Source |
|---|---|
| `protocol` | `oo.pipeline.analyze-pg-result/v1` |
| `published` | library receipt, required false |
| `estimate_is_exact` | library receipt, required false |
| `project` | request project |
| `source` | request source |
| `config_digest` | request digest |
| `source_generation` | request generation |
| `statement_timeout_ms` | request timeout |
| `lock_timeout_ms` | request timeout |
| `disposition` | `reusable` or `fresh_connection` |
| `tables` | one object per requested table, in request order |

Each table object has `namespace`, `name`, `physical_name`, `reltuples_bits`, and `estimated_rows`. `physical_name` is the stored mapping identifier. `reltuples_bits` is `f32::to_bits` of `pg_class.reltuples` after `ANALYZE`. `estimated_rows` is the library's half-away-from-zero rounding. It is not an exact row count. The receipt is not publication and not a column grant.

A wrong CA fails at exit 4 and does not change `reltuples`. A runtime member fails at exit 5 with `runtime caller`. A login that is not a superuser and cannot `MAINTAIN` the mapped shadow fails at exit 5 with `target role`. A generation that is not the stored maximum fails at exit 6 with `stale generation`. A logical table with no mapping fails at exit 6 with `mapping absent`. Those refusals are the existing `analyze_mapped_shadows` results. This command does not add a lock-timeout, forged-relation, or statement-timeout campaign.
