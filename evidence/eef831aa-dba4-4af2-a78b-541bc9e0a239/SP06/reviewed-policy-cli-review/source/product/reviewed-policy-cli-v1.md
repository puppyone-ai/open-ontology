# Reviewed policy command

Contract `oo.gateway.reviewed-policy-cli/v1`. Binary `reviewed-policy`, discovered from `crates/oo-gateway/src/bin/reviewed-policy.rs`.

The command reads one operator document from stdin. It calls `oo_gateway::reviewed_policy::compile_reviewed_policies` and writes the resulting `ColumnAclPolicy` set. The role, login, config digest, generation, and allowlist are the administrative binding from `contracts/gateway/reviewed-policy-v1.md`. They are not an authentication decision and they are not a grant.

`ColumnAclPolicy`, `LogicalTable`, `ColumnSpec`, `ClassStatus`, `PiiClass`, `RowFilter`, and `ReviewedBinding` do not implement `Serialize` or `Deserialize`. This command emits a DTO with those fields. A harness deserializes the DTO and builds `ColumnAclPolicy` values, then calls the existing `oo_gateway::policy::compile_column_acl_set`. `CompiledPolicySet` keeps its fields private and is not in the output.

The DTO uses the same names as those structs. Column `status` is `confirmed`, `auto_accepted`, or `unclassified`. `pii.class` is `none`, `unknown`, or `declared`. `declared` carries `explicit_allow`. `none` and `unknown` omit `explicit_allow`. `row_filter` is `null` or `{ "kind": "claims", "column", "claim" }`. That is the field set a `query-pg` policy object already deserializes in that command's private request type. This command does not run `query-pg`.

This compiler's successful policies use `status` `confirmed` and `row_filter` `null`. `pii` is `none` or `declared` with `explicit_allow` true. The DTO still has every field so the document is a full policy.

The command does not open a database, install a grant, publish a document, or write an audit WAL. It does not prove row-level security or a non-superuser grantor. A missing executor completion does not apply: this process has no session.

## Usage

No arguments. One JSON document on stdin. An argument exits 2 and prints one fixed usage line. The argument is not copied into that line.

Stdout on success is one compact JSON object and no trailing newline. Stderr is empty. On refusal, stdout is empty and stderr is one line. The line is `reviewed-policy: ` plus the token below. A decision line appends one reason word. Stderr does not include the operator document, a command body, a sample, a secret, or a `PolicyError` column name.

## Request

Stdin is read up to 1048577 bytes. A longer stream is not read further. More than 1048576 bytes is `budget` and is not parsed. The core scanner then accepts one value of at most 1048576 bytes. The scanner contract is `oo.gateway.reviewed-policy-cli/v1`. Duplicate keys, depth, node count, trailing data, and a non-UTF-8 stream are `scan`. `serde_json::from_str` is not the scanner.

The value is one object. `serde` `deny_unknown_fields` rejects any other field. A JSON null, a missing field, or a wrong JSON type is `request`. `protocol` must be `oo.gateway.reviewed-policy-cli/v1`. Any other string is `request`.

| Field | Shape |
|---|---|
| `protocol` | `oo.gateway.reviewed-policy-cli/v1` |
| `command` | string. The exact `reviewed-effective-oos` stdout, canonical JSON, no trailing newline |
| `artifact` | string. The captured-draft command JSON text passed to `compile_reviewed_policies` |
| `decisions` | string. The decision-array JSON text passed to `compile_reviewed_policies` |
| `binding` | object. One `ReviewedBinding` |

`command`, `artifact`, and `decisions` are strings, not nested objects. The command string is passed through unchanged, so the adapter's canonical-byte check sees the producer bytes. The artifact and decision strings are passed through unchanged. They do not have to be canonical. Key order in the operator object is not significant.

`binding` fields, all required:

| Field | Shape |
|---|---|
| `identity.project` | string |
| `identity.ontology_version` | string |
| `identity.catalog_version` | string |
| `identity.policy_id` | string |
| `identity.policy_version` | string |
| `identity.knowledge_id` | string |
| `identity.knowledge_version` | string |
| `seal.source_id` | string |
| `seal.schema` | string |
| `seal.schema_hash` | string |
| `seal.artifact_sha256` | string |
| `config_digest` | string |
| `generation` | JSON integer |
| `role` | string. The slug, not `oo_r_` plus the slug |
| `login` | string |
| `allow` | array of `{ "dataset", "column", "explicit_allow" }`. `explicit_allow` is a JSON boolean |

The CLI does not apply a second identifier grammar. `compile_reviewed_policies` and `compile_column_acl_set` do that. Three adapter documents near 1048576 bytes do not fit in this one operator document. That larger envelope is outside this command.

## Result

Success writes this object. Keys are in this order, compact, with no trailing newline.

| Key | Meaning |
|---|---|
| `protocol` | `oo.gateway.reviewed-policy-cli/v1` |
| `policies` | one object per allowlisted table, in the adapter's first-seen order |

Each policy object, in this order: `project`, `config_digest`, `generation`, `role`, `login`, `table`, `row_filter`.

`table` is `namespace`, `name`, `columns`. Each column is `name`, `status`, `semantic_type`, `pii`. `pii` is `class`, then `explicit_allow` only when `class` is `declared`.

`semantic_type` is the string on `ColumnSpec`. This command does not rename it. `row_filter` `null` is `None`. A claims object is `RowFilter::Claims`. This adapter returns `None`, so a successful document has `null`.

```json
{"protocol":"oo.gateway.reviewed-policy-cli/v1","policies":[{"project":"shop","config_digest":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","generation":4,"role":"clerk","login":"clerk_login","table":{"namespace":"public","name":"customer","columns":[{"name":"id","status":"confirmed","semantic_type":"identifier","pii":{"class":"none"}},{"name":"email","status":"confirmed","semantic_type":"email","pii":{"class":"declared","explicit_allow":true}}]},"row_filter":null}]}
```

The example names columns. It does not carry a cell sample.

A harness builds `ColumnAclPolicy` from each element and calls `compile_column_acl_set`. The same input yields the same bytes.

## Exits

| Code | Token | When |
|---|---|---|
| 0 | | The policy set was written. |
| 1 | `io` | Stdin could not be read, or stdout could not be written. |
| 2 | `usage` | Any argument. |
| 3 | `budget` | Stdin exceeds 1048576 bytes, or the adapter returned `Budget`. |
| 4 | `scan` | The operator document is not accepted JSON, or the adapter returned `Scan`. |
| 5 | `request` | The operator document's fields, types, or protocol string are refused. |
| 6 | `protocol` | The adapter returned `Protocol`. |
| 7 | `shape` | The adapter returned `Shape`. |
| 8 | `seal` | The adapter returned `Seal`. |
| 9 | `identity` | The adapter returned `Identity`. |
| 10 | `decision` | The adapter returned `Decision`. The line is `reviewed-policy: decision` plus one of `unknown`, `missing`, `stale_schema`, `stale_fact`, `rejected`, `unbound`, `mismatch`. |
| 11 | `semantic` | The adapter returned `Semantic`. |
| 12 | `pii_allow` | The adapter returned `PiiAllow`. |
| 13 | `duplicate` | The adapter returned `Duplicate`. |
| 14 | `forbidden` | The adapter returned `Forbidden`. |
| 15 | `rls` | The adapter returned `Rls`. |
| 16 | `policy` | The adapter returned `Policy`. |

A column that is in `hidden` after a successful bind is the adapter's `seal`, exit 8. This command does not invent a `hidden` decision reason.

## What stays open

The command does not authenticate the caller, the role, or the login. It does not install, grant, or publish. It does not write an audit WAL. It does not prove live row-level security or a non-superuser grantor. `query-pg` does not import this DTO. An installer harness can paste `policies` into its own request later. That paste is not this command.

M1-D14C and SP06-W01 stay open. This contract does not accept a parent plan item and it does not release.
