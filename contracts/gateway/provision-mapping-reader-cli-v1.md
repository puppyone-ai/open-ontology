# Provision mapping reader command

Contract `oo.gateway.provision-mapping-reader-cli/v1`. Binary `provision-mapping-reader`, discovered from `crates/oo-gateway/src/bin/provision-mapping-reader.rs`.

The command reads one operator document from stdin. It builds `ColumnAclPolicy` values and calls `oo_gateway::mapping_reader_access::provision_mapping_reader` on one TLS session. The explicit `login` is the login argument of that function. It is an administrative binding, not an authentication decision.

`ColumnAclPolicy` does not implement `Serialize` or `Deserialize`. Each policy object uses the same fields as the reviewed-policy command's policy DTO (`contracts/gateway/reviewed-policy-cli-v1.md`) and the install-column-acl command's policy DTO (`contracts/gateway/install-column-acl-cli-v1.md`). A harness may paste the `policies` array from a compiled `reviewed-policy` stdout into this document. The paste is not performed here. This command does not run `reviewed-policy` or `install-column-acl`. `CompiledPolicySet` stays private and is not in the output.

The library call grants the named login `USAGE` on schema `_oo`, `SELECT` on `_oo.physical_mappings`, and `EXECUTE` on `_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)`, or it returns when those privileges are already exact. It does not grant a business column, a table-level privilege on a shadow table, `EXECUTE` on `_oo.register_physical_mapping`, `PUBLIC`, a source object, or membership in `oo_target_owner`.

`SELECT` on `_oo.physical_mappings` is table-level, and the lookup function is `SECURITY DEFINER`. After a successful call the login can read every mapping row in that database, including rows outside the policy set. Those rows carry the source, config digest, generation, logical names, and physical `pt_` / `pc_` names. `information_schema.column_privileges` lists that table `SELECT` once per column of `_oo.physical_mappings`. That listing is the mapping catalog. It is metadata exposure. It is not scoped database isolation, and it is not a tenant boundary.

## Usage

No arguments. One JSON document on stdin. An argument exits 2 and prints one fixed usage line. The argument is not copied into that line. The password is not an argument and is not read from the environment.

Stdout on success is one compact JSON object and no trailing newline. Stderr is empty. On refusal, stdout is empty and stderr is one line. The line is `provision-mapping-reader: ` plus the token below. A policy line appends one reason word. Stderr does not include the operator document, a password, a driver message, a connection string, a column name, or a generation number.

## Request

Stdin is read up to 1048577 bytes. A longer stream is not read further. More than 1048576 bytes is `budget` and is not parsed. The core scanner then accepts one value of at most 1048576 bytes. The scanner contract is `oo.gateway.provision-mapping-reader-cli/v1`. Duplicate keys, depth, node count, trailing data, and a non-UTF-8 stream are `scan`. `serde_json::from_str` is not the scanner.

The value is one object. `serde` `deny_unknown_fields` rejects any other field. A JSON null is `request`, except `row_filter`, which may be null. A missing field or a wrong JSON type is `request`. `protocol` must be `oo.gateway.provision-mapping-reader-cli/v1`. Any other string, including `oo.gateway.reviewed-policy-cli/v1` and `oo.gateway.install-column-acl-cli/v1`, is `request`.

| Field | Shape |
|---|---|
| `protocol` | `oo.gateway.provision-mapping-reader-cli/v1` |
| `policies` | array of policy objects. The compiler allows 1 to 32 |
| `login` | lowercase identifier, at most 63 characters. Passed to `provision_mapping_reader` |
| `endpoint.host` | 1 to 253 ASCII letters, digits, dots, and hyphens. No empty label. No leading or trailing dot or hyphen |
| `endpoint.port` | 1 through 65535 |
| `endpoint.database` | lowercase identifier, at most 63 characters |
| `endpoint.user` | lowercase identifier, at most 63 characters. The TLS administrator, not the mapping-read login |
| `password_file` | path of a regular non-symlink |
| `ca_file` | path of a regular non-symlink |

Key order in the operator object is not significant. There is no `audit_file` field. Naming one is `request`.

Each policy object has `project`, `config_digest`, `generation`, `role`, `login`, `table`, and `row_filter`. `generation` is a JSON integer. `role` is the slug, not `oo_r_` plus the slug. `table` has `namespace`, `name`, and `columns`. Each column has `name`, `status`, `semantic_type`, and `pii`. `status` is `confirmed`, `auto_accepted`, or `unclassified`. `pii.class` is `none`, `unknown`, or `declared`. `declared` has `explicit_allow`. `none` and `unknown` omit `explicit_allow`. `row_filter` is null or `{ "kind": "claims", "column", "claim" }`. Any other `kind` is `request`.

This command does not apply a second identifier grammar. `compile_column_acl_set` does that, and it runs before a file is opened and before a connection. The explicit `login` must equal the compiled policy login. A difference is `binding` and does not open a file or a connection. A row filter, an empty set, a mixed identity, an unclassified column, or a metadata name is refused by that compiler on the same path.

`password_file` is at most 1026 bytes. One trailing LF, or one trailing CRLF, is removed. The remaining password is 1 to 1024 UTF-8 bytes and contains no NUL. If those bytes occur anywhere in the operator document, the command refuses `request` and does not connect. `ca_file` is at most 1 MiB. Empty input, or a PEM window containing `PRIVATE KEY`, is `tls`. A missing path, a symlink, a directory, or an oversize file is `request`. The connector is the existing `connect_require_tls`: one CA, aws-lc-rs, server-name verification, no native roots, and no certificate-skip flag.

## Result

Success writes this object. Keys are in this order, compact, with no trailing newline. The values are the `MappingReaderOutcome` fields. This command does not add a grantor or a physical-name list.

| Key | Meaning |
|---|---|
| `protocol` | `oo.gateway.provision-mapping-reader-cli/v1` |
| `effect` | `installed` or `replay` |
| `login` | login that was checked and, on install, granted |
| `role` | `oo_r_<slug>` whose installed column set matched the policies |

```json
{"protocol":"oo.gateway.provision-mapping-reader-cli/v1","effect":"installed","login":"clerk_login","role":"oo_r_clerk"}
```

The example shows the key order. It is not a cell sample.

`provision_mapping_reader` compiles the same slice again. A refusal from the first compile never connects. A stale generation, an unknown login, or a partial mapping privilege is refused inside the provision transaction, and that transaction rolls back. A partial privilege is not completed and is not revoked.

## Exits

| Code | Token | When |
|---|---|---|
| 0 | | The provision outcome was written. |
| 1 | `io` | Stdin could not be read, or stdout could not be written. |
| 2 | `usage` | Any argument. |
| 3 | `budget` | Stdin exceeds 1048576 bytes. |
| 4 | `scan` | The operator document is not accepted JSON. |
| 5 | `request` | The fields, types, protocol string, endpoint, or password file are refused. The password occurs in the document. |
| 6 | `tls` | The CA or the TLS session is refused. |
| 7 | `policy` | Compile or provision is refused. The line is `provision-mapping-reader: policy` plus one of `filter`, `identifier`, `classification`, `empty`, `stale`, `grantor`, `role`, `mismatch`, `login`, `mapping`, `physical`, `identity`, `limit`, `type`, `metadata`, `membership`, `database`, `binding`, `privilege`, `owner`. |

`stale` is `StaleGeneration`. `login` is `LoginAbsent`, including an unknown login. `binding` is `LoginMismatch`. `privilege` is `UnexpectedPrivilege`, including a partial mapping privilege. `owner` is `OwnerMembership`. `database` does not include the driver text. `type` does not include the column name.

## What stays open

The command does not authenticate the caller, the role, or the login. It does not decide tenancy. It does not issue credentials. The tested grantor is the fixture superuser `oo_fixture_admin`. A non-superuser grantor is not proven here.

The audit WAL, row-level security, and publication are open. This command does not write an audit row and does not write `pg_publication`. It does not `ANALYZE`. It does not call `query-pg`. A catalog load whose `unknown_rows` is `refuse` still refuses `reltuples` of `-1`.

The mapping grant is database-wide metadata exposure, as named above. Business column grants, `PUBLIC`, source grants, and `oo_target_owner` membership stay absent. This command does not add them and does not revoke a privilege it did not grant.

M1-D14F stays open. This contract does not accept a parent plan item and it does not release.
