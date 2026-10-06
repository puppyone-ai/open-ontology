# query-pg command

`query-pg` reads one `oo.gateway.query-pg/v1` document from a local file and runs the guarded `SELECT` statements in it on one dedicated TLS session. The file is an operator document on this machine. It is not an HTTP body, a bearer token, or an authentication claim. The command does not install grants, does not open a source database, and does not call the public parser worker.

The catalog this command loads is the author-approved policy catalog. That catalog is not independently reviewed. This command does not perform that review.

Stdout is quiet machine JSON. Each decoded row is one line. Each statement then ends with one summary line. Integers in a row are JSON strings. The command does not collect every row into a vector before writing. Exit 0 means every statement returned an executor summary whose status is `succeeded` and whose completion audit was synced. A truncated successful read is still exit 0.

## Usage

```text
query-pg --request <path>
query-pg --help
```

`--help` alone prints this contract's operator summary and exits 0. No arguments, an unknown flag, `--help` mixed with another argument, a missing path, or a non-UTF-8 path exits 2. The password is not an argument and is not read from the environment.

## Request

The file is a regular non-symlink of at most 1_048_576 bytes. `oo_core::parse_json_value` rejects duplicate keys, excess depth, excess nodes, and trailing bytes. `serde` rejects unknown fields. JSON null is rejected anywhere in the document. `serde_json` is built with `arbitrary_precision`. A field that does not fit its Rust type is a request failure.

`version` is `oo.gateway.query-pg/v1`.

`endpoint.host` is 1 to 253 ASCII letters, digits, dots, and hyphens, with no empty label and no leading or trailing dot or hyphen. `endpoint.port` is 1 through 65535. `endpoint.database` and `endpoint.user` are lowercase identifiers of at most 63 characters. The user is the login, not the `NOLOGIN` role.

`password_file` is a regular non-symlink of at most 1026 bytes. One trailing LF, or one trailing CRLF, is removed. The remaining password is 1 to 1024 UTF-8 bytes and contains no NUL. If those bytes occur anywhere in the request text, the command refuses the request. The password is not written to stdout, stderr, or the audit file.

`ca_file` is a regular non-symlink of at most 1 MiB. Empty input is refused. A PEM window containing `PRIVATE KEY` is refused. The connector is `rustls_connector`: one CA, aws-lc-rs, server-name verification, no native roots, and no certificate-skip flag.

`audit_file` is a path whose parent is an existing directory and whose final component does not exist. A symlink at that path is treated as existing. The command checks this before it connects. File mode is not a security claim. Permission deployment is out of scope.

`identity.project` is the physical-mapping source. `identity.policy_id` is the 64-character lowercase config digest. `identity.role` is the database role `oo_r_` plus the policy slug. Every policy in the list must share that project, digest, generation, login, and slug. The loader checks the same identity against the compiled policy. This command does not read `CompiledPolicy`'s private fields.

`policies` has 1 to 32 entries. Each entry is a column ACL: `project`, `config_digest`, `generation` (integer greater than 0), `role` (slug, not the `oo_r_` name), `login`, and `table`. `table.namespace`, `table.name`, and each column `name` are logical identifiers. A table has at most 512 columns. Column `status` is `confirmed`, `auto_accepted`, or `unclassified`. `semantic_type` is the classifier's label. `pii.class` is `none`, `unknown`, or `declared`. `declared` requires `explicit_allow`. `none` and `unknown` reject that field. `row_filter`, when present, must be `{ "kind": "claims", "column", "claim" }`. `compile_column_acl` refuses every row filter before a connection is opened.

`catalog.max_tables` is 1 to 32. `catalog.max_columns` is 1 to 512. `catalog.max_functions` is 1 to 8. `catalog.max_operators` is 54 to 128. `catalog.large_table_row_threshold` is an unsigned integer. `catalog.unknown_rows` is `refuse`, `zero`, or `saturate`.

`statement.source` is `request` or `stdin`. `request` requires `statement.sql`, a non-empty string of at most 65536 bytes with no NUL. `stdin` rejects a `sql` field and reads at most 256 KiB. The input is 1 to 8 non-empty lines. Each line is one `oo.gateway.query-pg-turn/v1` object, `{ "version", "sql" }`, with the same SQL bound. Turns share the request's role, catalog, and limits. This is not a conversation model.

`statement.max_rows` is 0 to 2000. `statement.max_result_bytes` is 0 to 262144. `statement.wall_clock_ms` is 1 to 30000. These are the executor's slice limits. The wall clock is the whole statement.

## Session

The command builds one rustls connector from the CA file and opens one `connect_require_tls` session. It polls that same connection while `load_policy_catalog` runs, then passes the connection to `DedicatedSession::bind`. The catalog load is one repeatable-read read-only snapshot. It does not select rows from a mapped user table. Type OIDs come from that PostgreSQL catalog. The command does not send fixture OID constants.

`execute_trusted_select` guards the original SQL in-process and runs the guarded statement as the restricted role. Unknown parser-worker admission stays blocked. Timeout, cancel, and drop behavior are the executor's. A guard refusal does not create the audit file and does not start user SQL.

## Stdout

Row line, in this key order:

```json
{"record":"row","turn":0,"cells":[{"kind":"null"},{"kind":"bool","value":true},{"kind":"int","value":"-9223372036854775808"},{"kind":"text","value":"beta"}]}
```

`kind` is `null`, `bool`, `int`, or `text`. `null` has no `value`. `bool` is a JSON boolean. `int` and `text` are JSON strings. `int` is the executor's decimal text, including the minimum int8 value.

Summary line, in this key order: `record` (`summary`), `turn`, `status` (`succeeded`, `partial`, or `failed`), `truncated`, `returned_rows`, `result_bytes`, `rollback_confirmed`, `socket_dropped`, `end_audit_ok`, `original_sha256`, `executed_sha256`, `stop`, `sqlstate`. Hashes are 64 lowercase hex digits from the admission record. `stop` is `timeout`, `canceled`, `type_refused`, `database`, or `protocol`, or JSON null. `sqlstate` is five ASCII uppercase letters or digits, or JSON null. Each line is flushed.

## Audit

The audit file is a local JSONL receipt. It is not a production audit WAL and it is not SP06-W06. The first admitted statement creates the file with `create_new` and append. Later statements in the same process append. `admit` validates the hashes, fingerprint, backend pid, and backend start, then `write_all`, `flush`, and `sync_all` before it returns. The executor starts user SQL only after `admit` returns. `complete` appends the outcome and syncs again. A failed `admit` returns `AuditUnavailable` and the executor does not run the statement. A failed `complete` leaves `end_audit_ok` false and does not turn a failure into success.

Both records use `protocol` `oo.gateway.query-pg-audit/v1`. They carry the role, project, policy id, both hashes, the fingerprint, the backend pid, and the backend start. The completion record also carries status, truncation, row and byte counts, rollback, socket drop, stop, and sqlstate. The file does not contain SQL text or the password.

## Exits

| Code | Token | When |
| --- | --- | --- |
| 0 | | Every turn succeeded and `end_audit_ok` is true. |
| 1 | `io` | Stdout could not be written. |
| 2 | `usage` | Arguments are not the usage above. |
| 3 | `request` | The document, a path, the password file, or stdin is refused. |
| 4 | `tls` | The CA or the TLS session is refused. |
| 5 | `policy` | Compilation or `load_policy_catalog` is refused. |
| 6 | `guard` | The guard refused the statement. No audit file is created. |
| 7 | `audit` | The audit path, admission, or completion sync is refused. User SQL does not start when admission fails. An otherwise succeeded statement with `end_audit_ok` false uses this code. |
| 8 | `query` | The executor reported a role, limit, type, timeout, database, or session failure, or a non-succeeded status. |

Stderr is `query-pg: ` plus the token. It does not include the password, SQL text, a driver message, or a connection string. Success writes no stderr.
