# Artifact diff

Contract `oo.builder.artifact-diff/v1`. Command `ontology-artifact-diff`.

This command reads two explicit `oo.builder.captured-pg-draft-command/v1` artifacts and writes one structural diff. The artifacts are the stdout documents of `oo-builder captured-pg-draft` and `oo-builder review-pg-draft`. Those commands call `compile_captured_pg_catalog`. This command does not compile a catalog, does not open a connection, and does not call a model.

The diff uses the fields those artifacts already carry. It does not invent a second ontology schema. It does not accept, reject, override, promote, grant, certify, or publish. Unknown classification stays unknown. Generated values and curated values stay in separate layers. A curated classification or review is not copied into the generated layer or the effective layer, and the effective layer is not read.

## Inputs

`--before <file>` and `--after <file>` are required, once each, in either order. Each path is a trusted local path to one regular file. A symlink, directory, or other non-regular file is refused. The file is at most 1048576 bytes. The reader stops at 1048577 bytes and does not read further. There is no stdin document. An unknown argument, a missing flag, a repeated flag, or an extra positional is usage.

The bytes are UTF-8. JSON is parsed with the core scanner: duplicate keys fail, a second value fails, depth is at most 32, the node count is at most 50000, and the document is at most 1048576 bytes. Messages do not echo the file path or the document.

The command object has exactly these keys:

| Key | Shape |
|---|---|
| `protocol` | `oo.builder.captured-pg-draft-command/v1` |
| `wrapper` | `oo.builder.captured-pg-draft/v1` object |
| `evidence_envelope` | `oo.builder.catalog-evidence/v1` object |
| `draft` | `oo.builder.structure-draft/v1` object |

Both sides must carry this same frozen set. A different protocol, compiler, or historical contract is `version.unsupported`. The sides are not diffed across versions.

| Field | Frozen value |
|---|---|
| `wrapper.protocol` | `oo.builder.captured-pg-draft/v1` |
| `wrapper.compiler` | `oo-builder/captured-pg-draft/v1` |
| `wrapper.connector_protocol` | `oo.source.pgsql/v1` |
| `wrapper.historical.protocol` | `oo.builder.structure-draft/v1` |
| `wrapper.historical.compiler` | `oo-builder/structure-draft/v1` |
| `wrapper.historical.pg_catalog_contract` | `oo.source.pgsql/v1` |
| `wrapper.historical.pg_catalog_contract_commit` | `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` |
| `wrapper.historical.pg_catalog_integration` | `not_accepted` |
| `draft.protocol` | `oo.builder.structure-draft/v1` |
| `draft.compiler` | `oo-builder/structure-draft/v1` |
| `evidence_envelope.protocol` | `oo.builder.catalog-evidence/v1` |
| `draft.provenance.pg_catalog_contract_commit` | `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` |
| `draft.provenance.pg_catalog_integration` | `not_accepted` |
| `draft.provenance.oos_version` | `oos/v1` |

`publication` is `unavailable`, `certification` is `none`, `llm` is `not_used`, and `source_connection` is `not_used` on both the wrapper and `draft.provenance`. `wrapper.live_catalog_review` is `pending`, `wrapper.row_import` is `not_proven`, and `wrapper.grants` is `not_authorized`. Any other status is `status.refused`. The value is not echoed.

Unknown keys fail on the command object, the wrapper, `wrapper.historical`, the evidence envelope's top level, `bindings`, `source`, `versions`, the draft's top level, `provenance`, and the physical, generated, curated, review, and queue objects this command reads. Keys inside a curated `classification` or `review` value are not an allowlist. That value is caller JSON stored by the structure-draft echo. It is hashed and not copied. The captured catalog object inside the evidence envelope stays bound by its hash and is not copied into the diff. Owner, privilege, comment text, default text, policy text, label, description, and `by` are not diff fields.

## Hashes

Canonical JSON sorts object keys, keeps array order, and escapes strings the same way the structure-draft compiler does. Digests are SHA-256 from `oo_core::canonical::hash_bytes`, hex-encoded by `oo_core::api::payload_sha256`.

Before a diff is built, each artifact must match its own seals:

| Check | Bytes |
|---|---|
| `wrapper.draft_hash` | canonical `draft` |
| `wrapper.evidence_envelope_hash` | canonical `evidence_envelope` |
| `wrapper.wrapper_hash` | canonical wrapper with `wrapper_hash` removed |
| `wrapper.manifest_hash` and `draft.provenance.manifest_hash` | canonical draft with `provenance.manifest_hash` removed |

`wrapper.schema_hash`, `config_hash`, `type_hash`, `evidence_hash`, `source_id`, and `database` must equal the same fields on `draft.provenance`. A miss is `hash.mismatch`. The message does not include the hash or the document.

A repeated structural identity in one artifact is `duplicate.fact`. Identities are not array indexes.

## Facts

Identities come from the draft that was compiled:

| Kind | Id | Layer | Payload |
|---|---|---|---|
| `source` | `source/{source_id}/{database}` | `physical` | `source_id`, `database`, `schema_hash`, `config_hash`, `type_hash`, `evidence_hash` |
| `table` | `table/{schema_ident}.{name_ident}` | `physical` | `schema`, `name`, `schema_ident`, `name_ident`, `relkind`, `replica_identity`, `rls_enabled`, `rls_forced` |
| `column` | `column/{schema_ident}.{name_ident}/{ident}` | `physical` | `attnum`, `name`, `ident`, `type_name`, `type_oid`, `type_schema`, `typmod`, `typtype`, `not_null`, `collation_oid`, `base_type_oid`, `numeric_precision`, `numeric_scale`, `identity`, `generated`, `enum_label_count`, `enum_labels_sha256`, `type_reason` |
| `column` | `column/{dataset}/{name}` | `generated` | `name`, `data_type`, `status`, `provenance` |
| `primary_key` | `primary_key/{schema_ident}.{name_ident}` | `physical` | ordered `columns` of `attnum`, `ident`, `name`, plus `nulls_not_distinct` |
| `primary_key` | `primary_key/{dataset}` | `generated` | ordered `columns` names |
| `foreign_key` | `foreign_key/{schema_ident}.{name_ident}/{fk_ident}` | `physical` | `name`, ordered `columns` of local and referenced idents and names |
| `foreign_key` | `foreign_key/{from_dataset}/{name}` | `generated` | `name`, `cardinality`, `join_hint`, `provenance`, `status`, from and to dataset and columns |

`generated` on a physical column is the captured PostgreSQL generated-column marker. An empty string means unset. It is not the generated layer. `identity` is the captured PostgreSQL identity marker. An empty string means unset.

A physical primary key or generated primary key that is absent or empty is not an identity. Foreign-key pair order and primary-key member order stay the captured order. `type_support.converted` must be false. `type_reason` is `type_support.reason`.

Column, primary-key, and foreign-key identities use the idents already stored on the artifact. This command does not recompute `pgident/v1`. A rename changes the ident, so the old column identity is removed and the new one is added.

Classification and review use the same status words and are not schema facts:

| Kind | Id | Layer | Rule |
|---|---|---|---|
| `classification` | `classification/{dataset}/{column}` | `generated` | JSON null is unknown. Unknown versus unknown is omitted. A non-null value stays in this layer. |
| `classification` | `classification/{dataset}/{column}` | `curated` | Present only when that curated column has a non-null `classification`. The structure-draft echo stores the caller object, so `datasets` or `relationships` may be absent. A missing array is empty. |
| `review` | `review/{dataset}/{column}` | `generated` | JSON null is unknown and is omitted when both sides are unknown. |
| `review` | `review/{dataset}/{column}` | `curated` | Present only when that curated column has a non-null `review`. |
| `review` | the decision `item_id` | `decision` | `action`, `schema_hash`, `fact_fingerprint` from `draft.reviews`. |
| `review` | the queue `item_id` | `queue` | A `review_queue` item whose `kind` is `stale_decision`. |

`action` must be `accept`, `reject`, or `override`. Reporting that token does not perform it. Every queue item has `publishable` false. A true value is `status.refused`.

A non-null classification or review value is reported as `unknown: false`, `value_sha256` of its canonical JSON, and `state` only when `state` is a string matching `^[a-z][a-z0-9_]{0,31}$`. `by`, `at`, labels, and descriptions are not copied.

## Status

`added`: the identity is only in the after artifact.

`removed`: the identity is only in the before artifact.

`changed`: the identity is in both and the payload differs.

`stale`: the identity is in both, the payload is equal, and `draft.provenance.schema_hash` differs. The fact did not change. The schema binding did, so a prior decision is not fresh. This follows the structure-draft rule that a schema-hash change makes every decision stale. It does not delete the fact.

The source payload includes `schema_hash`. A hash move is `changed` on the source, not `stale`.

Exceptions:

- Unknown versus unknown classification or review is unchanged even when the schema hash differs. Unknown is not a binding.
- A review decision is `stale` when its `schema_hash` is not the after artifact's `provenance.schema_hash`. A decision that exists only on the before side is `stale` when the document schema hash moved, and `removed` when it did not.
- A `stale_decision` queue item on the after artifact is `stale`. The same item only on the before side is `removed`.

Equal identities that are not stale are counted and omitted. Unique keys and policies are not identities. A schema-hash move they cause still changes the source binding and marks equal table, column, primary-key, and foreign-key facts stale. Enum label text is not copied. `enum_label_count` and `enum_labels_sha256` on the physical column show that the label set changed.

Facts are ordered by status, kind, id, then layer. Those fields sort alphabetically: status `added`, `changed`, `removed`, `stale`.

## Result

Stdout is one JSON object, canonical, with no trailing newline. It is parsed before it is written. `diff_hash` is `payload_sha256` of that document with `diff_hash` omitted, then the field is inserted. That is the same seal rule as `wrapper_hash`.

| Key | Value |
|---|---|
| `protocol` | `oo.builder.artifact-diff/v1` |
| `compiler` | `ontology-artifact-diff/v1` |
| `auto_accept` | false |
| `publication` | `unavailable` |
| `certification` | `none` |
| `grants` | `not_authorized` |
| `schema_hash_equal` | the two `provenance.schema_hash` values are equal |
| `before`, `after` | `source_id`, `database`, `schema_hash`, `config_hash`, `type_hash`, `evidence_hash`, `draft_hash`, `manifest_hash`, `wrapper_hash` |
| `counts` | `added`, `changed`, `removed`, `stale`, `unchanged` |
| `facts` | the omitted-unchanged list |
| `diff_hash` | the seal |

Each fact is `{id, kind, layer, status, before, after}`. A missing side is null.

A failure writes one static line to stderr and writes nothing to stdout.

| Exit | Cause |
|---|---|
| 0 | The diff was written |
| 1 | A file could not be opened or read, was not a regular file, was not UTF-8, or the scanner, shape, version, status, hash, or duplicate-identity rule rejected it, or stdout could not be written |
| 2 | Usage |

Usage stderr is exactly:

```
ontology-artifact-diff: usage: ontology-artifact-diff --before <file> --after <file>
ontology-artifact-diff: reads two oo.builder.captured-pg-draft-command/v1 artifacts
ontology-artifact-diff: writes one oo.builder.artifact-diff/v1 structural diff
ontology-artifact-diff: does not connect, call a model, import rows, accept, grant, certify, or publish
```

On exit 1 the line is `ontology-artifact-diff: before: oo.builder.artifact-diff/v1/{code} {message}` or the same with `after`, or without a side when the failure is not one file. `{code}` is `io.open`, `io.regular`, `io.read`, `json.utf8`, `json.syntax`, `json.duplicate_key`, `json.budget.bytes`, `json.budget.depth`, `json.budget.nodes`, `field.missing`, `field.unknown`, `field.invalid`, `version.unsupported`, `status.refused`, `hash.mismatch`, `duplicate.fact`, or `io.write`. The messages are the static sentences in the implementation. They do not include a path, a hash, or document text.

This slice does not mark SP07-W05, owner authority, authentication, or publication complete.
