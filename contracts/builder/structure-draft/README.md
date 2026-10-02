# Structure draft

Contract `oo.builder.structure-draft/v1`. Compiler id `oo-builder/structure-draft/v1`.

This slice turns one bounded catalog-evidence envelope into a reviewable structural Ontology Object Schema (OOS) draft. The implementation is `compile_structure_draft` in `crates/oo-builder`. The binary subcommand `structure-draft` reads one JSON document from stdin and writes the canonical draft bytes to stdout. No arguments. Exit 0 on success, exit 1 on a draft error, exit 2 for a missing or unknown command.

The only supported adapter in this slice is `pgsql`. The adapter is a pure parser of an already captured catalog document. It follows the frozen `oo.source.pgsql/v1` field set and `pg-catalog-hash/v1` preimage from commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. `provenance.pg_catalog_integration` is `not_accepted`. This crate does not depend on `oo-connectors` and does not open a database connection.

This component does not finish SP07. It does not sample rows, apply semantic rules, call a model, open a review API, load role packs, publish, grant, certify, or answer queries.

## Envelope

Protocol `oo.builder.catalog-evidence/v1`. JSON is parsed with the core scanner: duplicate keys fail, a second value fails, depth is at most 32, the node count is at most 50000, and the document is at most 1048576 bytes. Unknown fields fail. Messages do not echo catalog text.

| Field | Rule |
|---|---|
| `adapter` | `pgsql` in this slice. Any other value is `adapter.unsupported`. |
| `source.source_id` | `^[a-z][a-z0-9_-]{0,62}$`. Must equal the catalog source. |
| `source.database` | Must equal the catalog database. |
| `versions.schema` | `pg-catalog-hash/v1` |
| `versions.config` | `oo.config/v1` |
| `versions.type` | `oo.source.pgsql/v1` |
| `versions.evidence` | `oo.builder.catalog-evidence/v1` |
| `bindings` | `schema_hash`, `config_hash`, `type_hash`, `evidence_hash`. Each is 64 lowercase hex and must equal the recomputed digest. |
| `selection` | Exact set of catalog tables, columns, and keys. A mismatch is an error. It is not a silent filter. |
| `catalog` | One `oo.source.pgsql/v1` document. |
| `reviews` | Optional. Absent means none. |
| `curated` | Optional. Absent becomes an empty overlay echo. |

`selection.tables[].keys` uses the token `primary_key` for the primary key and the constraint name for each unique key and foreign key. A constraint whose name is the token `primary_key` is rejected.

Stable identifiers use `pgident/v1`. Names are canonical NFC UTF-8. Empty names and NUL fail. A safe name is lowercase `[a-z0-9_]`, starts with a letter or underscore, is at most 63 bytes, is not a PostgreSQL 17 reserved word, and does not start with `q_`. Every other name is `q_` plus the lowercase hex of the NFC bytes. Different originals that land on one ident fail `ident.collision`. Schema idents are global, table idents are unique in a schema, and column idents are unique in a table. This slice also rejects ASCII controls and names longer than 63 bytes. It does not truncate. An ident that does not fit `SourceName` or `TableName` stays in the physical section and is queued as `identifier_not_in_oos`. Item ids never use array indexes.

NFC is Unicode 17.0.0 canonical composition from `unicode-normalization` 0.1.25, the same version the PostgreSQL connector uses. It is not NFKC and it does not case-fold. ASCII names, and names the old hand-written composer already got right (`é`, Hangul, `客户`), keep the same stable ident. That composer blocked a second mark of an equal combining class, so `U+0045 U+0304 U+0300` and `U+1E14` became `U+0112 U+0300` instead of `U+1E14`. It also gave 34 Unicode 17 marks class 0, so `U+0065 U+1ACF U+0323` stayed unchanged instead of `U+1EB9 U+1ACF`. Identifiers and any schema, evidence, or manifest digest that contains those corrected strings differ from a draft built by the old composer. Recompile from the raw catalog and review the result. An old digest is not a fresh binding, and this compiler does not rewrite a human decision onto the new digest. It does not rename a target, drop a column, or widen a permission because a hash moved. Raw source names in the catalog stay the captured strings. The checked-in goldens for this contract use names whose NFC did not change, so those files are not regenerated.

Physical types stay explicit. `type_support.converted` is false. `type_conversion` is `unsupported`. A domain stays `domain_not_converted`. An enum stays `enum_not_converted`. `numeric` with `typmod < 0` or precision above 38 stays `unbounded_or_wide_numeric`. Builtin and pseudo types stay `known_builtin_not_converted`. Anything else stays `unknown_type_oid`. Precision and scale come from `typmod`: values below -1 fail; -1 has no precision; otherwise precision is `(typmod - 4) >> 16` and scale is `(typmod - 4) & 0xFFFF`. The OOS `data_type` is the physical type name only for a known builtin that fits a label, including a bounded `numeric`. Domain, enum, wide numeric, and unknown types omit `data_type`.

Comments, defaults, policy expressions, and domain checks are `{byte_len, sha256}` only. `byte_len` 0 uses the empty SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. Claimed length above 65536 fails. Bodies are not stored and are not copied into agent metadata. Privileges and owner are validated and discarded. Enum label text is part of the schema-hash preimage only. The draft stores the label count and the SHA-256 of the canonical JSON array of labels.

## Hashes

`schema_hash` is SHA-256 of the `pg-catalog-hash/v1` preimage. That preimage is a hand-built JSON string, not sorted-key canonical JSON. Tables are ordered by `(schema, name)`. This adapter writes columns in `attnum` order. Unique keys, foreign keys, and policies are ordered by name. Primary-key member order is significant. The preimage includes physical names, type facts, and raw enum labels. It excludes comments, privileges, owner, and `captured_at`. The empty document `{"tables":[]}` hashes to `cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc`.

`config_hash` is the sorted-key SHA-256 of `{"adapter":"pgsql","config_version":"oo.config/v1","database","source_id"}`.

`type_hash` is the sorted-key SHA-256 of `type_conversion`, the type version, and one object per column sorted by schema, table, and `attnum`. Each object carries `attnum`, `base_type_oid`, precision, scale, reason, schema, table, type name, type oid, type schema, `typmod`, and `typtype`.

`evidence_hash` binds the evidence version, `schema_hash`, `type_hash`, source id, database, the sorted selection, and comment records `{path, byte_len, sha256}`. Comment paths are `table/{schema_ident}.{name_ident}` and `column/{schema_ident}.{name_ident}/{column_ident}`. `captured_at` is not in this hash. It is copied into provenance, so it changes `manifest_hash`.

Fact fingerprints are sorted-key SHA-256 of the original physical names. A column fingerprint covers `attnum`, name, `type_oid`, `typmod`, `typtype`, `not_null`, `collation_oid`, identity, generated, schema, and table. A primary-key fingerprint is `kind: primary_key` plus the ordered column names. A foreign-key fingerprint is `kind: foreign_key`, the constraint name, schema, table, and the ordered pairs `{name, ref_name, ref_schema, ref_table}`. A dataset fingerprint is `{kind: dataset, schema, name}`.

`manifest_hash` is the sorted-key SHA-256 of the draft with `provenance.manifest_hash` omitted, then inserted. Library bytes have no trailing newline. The CLI writes those bytes unchanged.

## Draft

Top-level keys: `protocol`, `compiler`, `physical`, `generated`, `curated`, `effective`, `reviews`, `review_queue`, `provenance`.

`physical` repeats tables, columns, keys, and policies as hash-only evidence. It has no owner, privilege, enum label text, comment text, or SQL.

`generated` is the structural OOS from the catalog. `curated` is the caller's overlay, canonicalized, with lists sorted by their stable keys. Primary-key member order is kept. `effective` is the core overlay merge of `generated` with only the fresh structural curated fields. Semantic fields stay in the curated echo and are not merged.

Known primary keys and foreign keys are structural facts and tentative candidates. Relationship provenance is `declared_fk`, status is `draft`, review is absent, confidence is null, and `join_hint` is `left`. Cardinality is `one_to_one` when the foreign-key column idents equal the child primary key or a unique key, and `many_to_one` otherwise. Two or more foreign keys between the same qualified pair stay separate and are queued `fk_ambiguous`. A parent outside the selection produces no relationship and is queued `fk_target_not_selected`. A referenced column list that is not a known key of a selected parent is still emitted when the names fit, with `fk_target_key_unverified`. Mixed referenced tables in one foreign key fail. Composite foreign keys keep constraint order. Separate foreign keys may name the same column. A repeated member inside one primary key, one unique key, or one foreign-key source list or referenced list is `duplicate.key_member`. The same column repeated by attnum, by name, or by an NFC alias of either is that error. Column, table, and schema ident collisions stay `ident.collision`. The message and path do not include the name or a comment. Hash checks run before this error, so a wrong binding stays `hash.schema`, `hash.config`, `hash.type`, or `hash.evidence`. `fixtures/duplicate-pk-member.json` is a bound envelope with one repeated primary-key member. `fixtures/fk-from-endpoint-rejected.json` rejects the source column of one foreign key; the effective draft omits that relationship and queues `relationship_endpoint_not_effective`.

Datasets are tier `raw`, `recommended` false, provenance `declared`, status `draft`. There is no entity, grain, row count, metric, or confidence. Publication is `unavailable`. Certification is `none`. `llm` is `not_used`. `source_connection` is `not_used`.

## Review

A decision is `{item_id, action, schema_hash, fact_fingerprint}`. Actions are `accept`, `reject`, and `override`. A decision is fresh only when both the schema hash and the fact fingerprint match. A schema-hash change makes every decision stale. A stale decision returns the structural item to `pending` and adds `stale_decision`. It does not delete or resurrect the fact. An unknown item whose schema hash matches is `review.unknown_item`. An unknown item whose schema hash differs is `stale_decision`.

Item ids:

| Id | Meaning |
|---|---|
| `column/{schema_ident}.{table_ident}/{column_ident}` | Structural column |
| `primary_key/{schema_ident}.{table_ident}` | Structural primary key |
| `relationship/{schema_ident}.{table_ident}/{fk_ident}` | Foreign key |
| `dataset/{schema_ident}.{table_ident}` | Dataset ident that does not fit OOS, or a curated dataset binding |
| `pii/...`, `semantic/...` | Unknown until a later review surface exists |
| `type/...` | Present only when conversion is unsupported |
| `classification/...`, `review/...`, `entity/...` | Semantic fields that this slice does not apply |

Decisions aimed at `pii/`, `semantic/`, `type/`, `classification/`, `review/`, `entity/`, or a dataset rejection are `review.not_in_scope`. `accept` marks the structural queue item `accepted`. It does not apply curated fields, classify PII, or make the item publishable. `reject` requires a curated `{name, status: rejected}` or an empty `primary_key`, and removes that item from `effective` only. `override` requires the curated structural fields or it is `review.override_missing`.

Structural curated fields that may be applied are `label`, `description`, `status` (`draft` or `rejected`), `grain`, `source_name`, `unit`, and `primary_key`. `pii`, `semantic_type`, `classification`, `review`, and `entity` stay in the echo and are queued `semantic_field_not_applied`. A curated dataset, column, or relationship that is not in the generated draft is `curated.unknown_item`. A structural curated field with no fresh decision is `curated_unbound` and is not applied. After a column rejection, a dangling effective primary-key name is removed from `effective` and queued `primary_key_column_not_effective`. The same fresh rejection removes an effective relationship when any source or referenced column is absent from the effective dataset, including one member of a composite key, a self-reference, or both ends. The queue item is `relationship_endpoint_not_effective` with detail `from_column_not_effective`, `to_column_not_effective`, `from_and_to_column_not_effective`, or `endpoint_dataset_not_effective`. That item is `pending` and `publishable` false. Generated evidence, the curated echo, and the historical review decision stay. A stale column rejection removes neither the column nor the relationship. An unrelated relationship stays. Accepting a relationship does not keep it in `effective` after its column is rejected, and it does not rewrite that accept.

Queue items are `{item_id, kind, state, publishable, fact_fingerprint, detail}`. `publishable` is false. States are `pending`, `accepted`, and `rejected`. The queue is sorted by item id, then kind, and duplicate pairs are dropped.

Queue kinds: `structural_column`, `structural_primary_key`, `candidate_relationship`, `fk_ambiguous`, `fk_target_not_selected`, `fk_target_key_unverified`, `pii_unknown`, `semantic_unknown`, `type_unsupported`, `stale_decision`, `curated_unbound`, `identifier_not_in_oos`, `semantic_field_not_applied`, `primary_key_column_not_effective`, `relationship_endpoint_not_effective`.

## Budgets

| Limit | Value |
|---|---|
| Tables | 64 |
| Columns on one table | 256 |
| Keys on one table, and the selection key list | 32 |
| Columns in one key | 16 |
| Policies on one table | 32 |
| Privilege entries accepted and discarded | 64 |
| Enum labels | 64 |
| Domain checks | 16 |
| Review decisions | 1024 |
| Curated datasets, relationships, and columns | 64 |
| Physical name | 63 bytes |
| Untrusted byte length | 65536 |
| Item id | 512 bytes |
| `attnum` | 1 through 1600 |
| `type_oid` | 1 through 4294967295 |

The one-byte-over document and the depth-33 document are refused by the core scanner. Those inputs are constructed in the oo-builder tests. They are not stored as fixtures.

## Errors

Codes are `{contract}/{name}`. The contract is `oo.builder.catalog-evidence/v1` except when the core scanner already produced a full code. The display form is `{code} at {path}: {message}`. The message is a static rule name.

Names used by this slice include `adapter.unsupported`, `source.mismatch`, `version.unsupported`, `field.unknown`, `field.missing`, `field.duplicate`, `field.invalid`, `schema.invalid`, `hash.schema`, `hash.config`, `hash.type`, `hash.evidence`, `selection.mismatch`, `duplicate.selection`, `duplicate.key_member`, `ident.empty`, `ident.control`, `ident.collision`, `ident.mismatch`, `budget.tables`, `budget.columns`, `budget.keys`, `budget.key_columns`, `budget.policies`, `budget.privileges`, `budget.enum_labels`, `budget.domain_checks`, `budget.name_bytes`, `budget.reviews`, `budget.curated`, `budget.item_id`, `json.duplicate_key`, `json.budget.bytes`, `json.budget.depth`, `json.budget.nodes`, `json.syntax`, `review.action`, `review.not_in_scope`, `review.unknown_item`, `review.override_missing`, `curated.invalid`, `curated.unknown_item`, `oos.invalid`, and `io.read`.

## Left for later work items

SP07-W01 overlay and semantic diff, beyond the core merge used here, including AT-OOS-01. SP07-W02 profile, cache, sample, time window, and statistics. SP07-W03 rules, PII, enums, expert cards, and auto-accept. SP07-W04 model proposals. SP07-W05 review API, console, owner RBAC, and classification publication. SP07-W06 role packs. Real PostgreSQL import, approved-draft integration, and query integration are separate. This slice does not mark SP07 or the vertical flow complete.
