# Captured PostgreSQL draft

Contract `oo.builder.captured-pg-draft/v1`. Compiler id `oo-builder/captured-pg-draft/v1`.

`compile_captured_pg_catalog` takes the serde JSON of one `oo.source.pgsql/v1` catalog, the document `PgSession::catalog` and `SnapshotOwner` already produce, and returns an evidence envelope plus a structure draft. The builder does not open a connection and does not depend on `oo-connectors`. This wrapper does not finish SP07. It does not sample rows, apply semantic rules, call a model, publish, grant, or certify.

## Input

The catalog is parsed with the core scanner and the existing `oo.source.pgsql/v1` parser. Duplicate keys fail. Depth, node count, and the 1048576-byte ceiling are the scanner limits. Unknown fields fail. `type_support.converted: true` is `type.converted`. A `schema_hash` that is not the recomputed `pg-catalog-hash/v1` digest is `hash.schema`. An unsupported protocol, hash algorithm, ident algorithm, core contract, or type conversion is `version.unsupported`.

The caller does not pass `config_hash`, `type_hash`, `evidence_hash`, or a selection. Those are derived from the catalog by the same functions the structure-draft compiler uses. The selection is the exact table, column, and key set. The primary-key token is `primary_key`. Unique keys and foreign keys contribute their constraint names.

Optional `curated_json` and `reviews_json` are the overlay and the decision list already defined by `oo.builder.structure-draft/v1`. They are scanned before they are placed in the envelope. Absent means none.

Before hashing, tables are ordered by `(schema, name)`, columns by `attnum`, and unique keys, foreign keys, and policies by name. Privileges are ordered by grantee, privilege, and grant option. Primary-key member order, foreign-key pair order, enum label order, and domain-check order stay as captured. That normalization changes no `pg-catalog-hash/v1` digest. It makes an equivalent reorder produce the same envelope bytes.

Comments, defaults, policy expressions, and domain checks stay `{byte_len, sha256}`. A comment body is an unknown field and is rejected. The body is not copied into the envelope, the draft, or the wrapper. Privileges and owner stay in the captured catalog document inside the envelope. The structure draft still validates them and leaves them out of the OOS layers.

## Result

`CapturedPgDraft` carries three canonical JSON strings, with no trailing newline:

| Field | Bytes |
|---|---|
| `evidence_envelope` | `oo.builder.catalog-evidence/v1`, passed to `compile_structure_draft` |
| `draft` | `oo.builder.structure-draft/v1` from that compiler |
| `wrapper` | This contract |

`schema_hash`, `config_hash`, `type_hash`, and `evidence_hash` are the recomputed bindings. `captured_catalog_hash` is the sorted-key SHA-256 of the normalized catalog value. It is not the schema hash. `evidence_envelope_hash` and `draft_hash` are SHA-256 of those exact byte strings. `manifest_hash` is the draft seal. `wrapper_hash` is the sorted-key SHA-256 of the wrapper with `wrapper_hash` omitted; the field is then inserted. That is the same seal rule as `manifest_hash`.

The draft keeps the historical compiler contract. `provenance.pg_catalog_contract_commit` remains `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. `provenance.pg_catalog_integration` remains `not_accepted`. Those fields are the compiler's existing statement. They are not a new live approval, and this wrapper does not change them or any previously sealed draft hash. A draft that does not echo that contract is refused.

The wrapper records the same historical pair under `historical`, plus:

| Field | Value | Meaning |
|---|---|---|
| `connector_protocol` | `oo.source.pgsql/v1` | Catalog document this wrapper accepts |
| `live_catalog_review` | `pending` | Independent review of this capture has not happened |
| `row_import` | `not_proven` | A catalog snapshot is not a complete row import |
| `grants` | `not_authorized` | Capture does not grant or prove publication rights |
| `publication` | `unavailable` | |
| `certification` | `none` | |
| `llm` | `not_used` | |
| `source_connection` | `not_used` | |

`live_catalog_review` stays `pending` when the inner draft applies a caller decision. An accept or override is the existing review queue. It does not turn `not_accepted` into a live catalog approval.

Generated, curated, and effective layers, the review queue, unknown PII and semantics, stale schema and fact decisions, foreign-key endpoint closure, and Unicode 17 canonical identifiers are unchanged. They are the structure-draft compiler's behavior. A schema-hash change returns a prior decision to pending, queues `stale_decision`, and leaves the curated echo in place without applying it.

## Command

`oo-builder captured-pg-draft` reads one `oo.source.pgsql/v1` catalog document from stdin and writes one command result to stdout. Protocol `oo.builder.captured-pg-draft-command/v1`. No arguments. The command calls `compile_captured_pg_catalog(input, None, None)`. It does not open a connection, call a model, read source credentials, import rows, grant, certify, or publish.

Curated overlays and review decisions are not arguments of this command. They stay on `compile_captured_pg_catalog`.

Stdout is one JSON object with sorted keys and no trailing newline. `wrapper`, `evidence_envelope`, and `draft` are objects, the same values as the three canonical strings on `CapturedPgDraft`. They are not escaped JSON strings. The bytes of each object are those strings.

| Key | Value |
|---|---|
| `protocol` | `oo.builder.captured-pg-draft-command/v1` |
| `wrapper` | Parsed `CapturedPgDraft.wrapper` |
| `evidence_envelope` | Parsed `CapturedPgDraft.evidence_envelope` |
| `draft` | Parsed `CapturedPgDraft.draft` |

The captured catalog inside the envelope still carries owner and privilege records. The draft and the wrapper do not copy the owner. Comment bodies are not in the result. `wrapper.row_import` is `not_proven`. `wrapper.grants` is `not_authorized`. Unknown PII, tentative foreign keys, and the review queue are the draft's.

Stdin is read up to 1048577 bytes. A longer stream is not read further. The parser then rejects a document above 1048576 bytes. Nothing is written to stdout until the command result parses as one JSON object. A failure writes a static diagnostic to stderr and does not echo the catalog.

| Exit | Cause |
|---|---|
| 0 | The command result was written |
| 1 | Stdin could not be read, is not UTF-8, the catalog or compiler rejected it, or stdout could not be written |
| 2 | The command is missing or unknown, or `captured-pg-draft` was given an argument |

`structure-draft` is unchanged: one evidence envelope on stdin, the same draft bytes on stdout, exit 0 on success, exit 1 on a draft error, and exit 2 when that command is given an argument. With no arguments the process still reports that the SP00-W01 entry skeleton is not implemented, and it names these two public draft commands.
