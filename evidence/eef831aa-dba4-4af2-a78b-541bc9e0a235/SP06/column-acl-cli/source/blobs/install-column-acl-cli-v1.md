# Install column ACL command

Contract `oo.gateway.install-column-acl-cli/v1`. Binary `install-column-acl`, discovered from `crates/oo-gateway/src/bin/install-column-acl.rs`.

The command reads one operator document from stdin. It builds `ColumnAclPolicy` values, calls `oo_gateway::policy::compile_column_acl_set`, and then `install_column_acl_set` on one TLS session. The role and login are the administrative binding already carried on each policy. They are not an authentication decision.

`ColumnAclPolicy` does not implement `Serialize` or `Deserialize`. The policy objects in this document use the same fields as the reviewed-policy command's policy DTO. That command's contract and source were untracked when this schema was frozen. This command does not import them and does not run that binary. A harness may paste the `policies` array from that command's stdout into this document. The paste is not performed here. `CompiledPolicySet` stays private and is not in the output.

The library install is unchanged. It creates or replays one `NOLOGIN` role and grants `USAGE` on schema `_oo` plus `SELECT` on the mapped allowlist. This command does not add a grant on `physical_mappings`, does not `ANALYZE`, does not write `pg_publication`, and does not write an audit WAL.

## Usage

No arguments. One JSON document on stdin. An argument exits 2 and prints one fixed usage line. The argument is not copied into that line. The password is not an argument and is not read from the environment.

Stdout on success is one compact JSON object and no trailing newline. Stderr is empty. On refusal, stdout is empty and stderr is one line. The line is `install-column-acl: ` plus the token below. A policy line appends one reason word. Stderr does not include the operator document, a password, a driver message, a connection string, or a column name.

## Request

Stdin is read up to 1048577 bytes. A longer stream is not read further. More than 1048576 bytes is `budget` and is not parsed. The core scanner then accepts one value of at most 1048576 bytes. The scanner contract is `oo.gateway.install-column-acl-cli/v1`. Duplicate keys, depth, node count, trailing data, and a non-UTF-8 stream are `scan`. `serde_json::from_str` is not the scanner.

The value is one object. `serde` `deny_unknown_fields` rejects any other field. A JSON null is `request`, except `row_filter`, which may be null. A missing field or a wrong JSON type is `request`. `protocol` must be `oo.gateway.install-column-acl-cli/v1`. Any other string, including `oo.gateway.reviewed-policy-cli/v1`, is `request`.

| Field | Shape |
|---|---|
| `protocol` | `oo.gateway.install-column-acl-cli/v1` |
| `policies` | array of policy objects. The compiler allows 1 to 32 |
| `endpoint.host` | 1 to 253 ASCII letters, digits, dots, and hyphens. No empty label. No leading or trailing dot or hyphen |
| `endpoint.port` | 1 through 65535 |
| `endpoint.database` | lowercase identifier, at most 63 characters |
| `endpoint.user` | lowercase identifier, at most 63 characters. The TLS login, not the `NOLOGIN` role |
| `password_file` | path of a regular non-symlink |
| `ca_file` | path of a regular non-symlink |

Key order in the operator object is not significant. There is no `audit_file` field. Naming one is `request`.

Each policy object has `project`, `config_digest`, `generation`, `role`, `login`, `table`, and `row_filter`. `generation` is a JSON integer. `role` is the slug, not `oo_r_` plus the slug. `table` has `namespace`, `name`, and `columns`. Each column has `name`, `status`, `semantic_type`, and `pii`. `status` is `confirmed`, `auto_accepted`, or `unclassified`. `pii.class` is `none`, `unknown`, or `declared`. `declared` has `explicit_allow`. `none` and `unknown` omit `explicit_allow`. `row_filter` is null or `{ "kind": "claims", "column", "claim" }`. Any other `kind` is `request`.

This command does not apply a second identifier grammar. `compile_column_acl_set` does that. A row filter, an empty set, a mixed identity, an unclassified column, or a metadata name is refused by that compiler before a file is opened and before a connection.

`password_file` is at most 1026 bytes. One trailing LF, or one trailing CRLF, is removed. The remaining password is 1 to 1024 UTF-8 bytes and contains no NUL. If those bytes occur anywhere in the operator document, the command refuses `request` and does not connect. `ca_file` is at most 1 MiB. Empty input, or a PEM window containing `PRIVATE KEY`, is `tls`. A missing path, a symlink, a directory, or an oversize file is `request`. The connector is the existing `connect_require_tls`: one CA, aws-lc-rs, server-name verification, no native roots, and no certificate-skip flag.

## Result

Success writes this object. Keys are in this order, compact, with no trailing newline.

| Key | Meaning |
|---|---|
| `protocol` | `oo.gateway.install-column-acl-cli/v1` |
| `effect` | `installed` or `replay` |
| `role` | database role, `oo_r_` plus the slug |
| `grantor` | `session_user` that issued the grants |
| `tables` | one object per input policy, in that order |

Each table object is `physical_table`, then `physical_columns`. Those names are the mapped shadow names returned by the library. This command does not invent them.

```json
{"protocol":"oo.gateway.install-column-acl-cli/v1","effect":"installed","role":"oo_r_clerk","grantor":"oo_fixture_admin","tables":[{"physical_table":"pt_example","physical_columns":["pc_example"]}]}
```

The example shows the key order. A live physical name is 63 characters. The example is not a cell sample.

`install_column_acl_set` compiles the same slice again. A refusal from the first compile never connects. A stale generation or a missing second mapping is refused inside the install transaction, and that transaction rolls back.

## Exits

| Code | Token | When |
|---|---|---|
| 0 | | The install outcome was written. |
| 1 | `io` | Stdin could not be read, or stdout could not be written. |
| 2 | `usage` | Any argument. |
| 3 | `budget` | Stdin exceeds 1048576 bytes. |
| 4 | `scan` | The operator document is not accepted JSON. |
| 5 | `request` | The fields, types, protocol string, endpoint, or password file are refused. The password occurs in the document. |
| 6 | `tls` | The CA or the TLS session is refused. |
| 7 | `policy` | Compile or install is refused. The line is `install-column-acl: policy` plus one of `filter`, `identifier`, `classification`, `empty`, `stale`, `grantor`, `role`, `mismatch`, `login`, `mapping`, `physical`, `identity`, `limit`, `type`, `metadata`, `membership`, `database`. |

`stale` is `StaleGeneration`. `mapping` is `MappingAbsent`. `database` does not include the driver text.

## What stays open

The command does not authenticate the caller, the role, or the login. It does not issue credentials. The tested grantor is the fixture superuser `oo_fixture_admin`. A non-superuser grantor is not proven here.

The audit WAL is open. This command does not write one. `query-pg` is a separate command. This command does not call it. A catalog load whose `unknown_rows` is `refuse` still refuses `reltuples` of `-1`, and this command does not `ANALYZE`.

Row-level security and publication are open. A row filter is refused before any grant. `pg_publication` is not written. A column that is not in the set, including `_oo_pos`, stays ungranted. The business grants are the role, the membership, schema `USAGE`, and column `SELECT` only.

M1-D14D stays open. This contract does not accept a parent plan item and it does not release.
