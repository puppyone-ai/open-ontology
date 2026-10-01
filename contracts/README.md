# Contracts

Checked-in models for SP00-W02 and SP00-W03. The Rust types live in `crates/oo-core`. These files are the generated schema, the API fixtures, and the synthetic source-cell fixtures. They are not a running server, a database driver, or a loader.

| Artifact | Contract | Contents |
|---|---|---|
| `schema/config-v1.json` | `oo.config/v1` | `schema_version: config/v1`. One document is `oo.yaml`, `sources.yaml`, `files.yaml`, `roles.yaml`, `agent.yaml`, `hooks.yaml`, or an eval suite. |
| `schema/oos-v1.json` | `oo.oos/v1` | `ontology.yaml` uses `spec_version: oos/v1`. Every other fragment uses `schema_version: oos/v1`. |
| `schema/batch-v1.json` | `oo.batch/v1` | Batch metadata. Protocol const `batch/v1`. |
| `schema/api-v1.json` | `oo.api/v1` | Query, page, accepted-operation, and export bodies. |
| `schema/error-v1.json` | `oo.api/v1` | `{error:{code,message,hint,retryable,query_id}}`. |
| `schema/openapi-v1.json` | OpenAPI 3.1 | Component schemas only. No operations. |
| `canonical-value-v1.md` | `oo.canonical/v1` | SHA-256 frame layout and independent vectors. |
| `fixtures/api/` | `oo.api/v1` | Legal JSON bodies. Exact amounts are strings. |
| `fixtures/pg-mysql/cases.json` | synthetic | Raw cells. `driver` is `none`. |

Regenerate the schema JSON from the Rust types:

```
OO_WRITE_SCHEMAS=1 cargo test -p oo-core --locked --test schema_contract schemas_match_the_rust_types_and_the_meta_schema -- --exact
```

A later run without `OO_WRITE_SCHEMAS` fails if the files drift. JSON Schema is draft 2020-12, compiled with file and network resolvers off. Unknown fields fail. `serde` and the schema must agree on shape. The schema is not the whole trust boundary.

`maxLength` counts Unicode scalars. `x-utf8-max-bytes` is the UTF-8 byte budget. Draft 2020-12 cannot express a byte length by itself, so a schema that only sets `maxLength` accepts a multibyte string the Rust constructor rejects. Rust validation at the parser is required. The custom keyword is an extra check when `oo-core` compiles the schema. It is not a claim that JSON Schema and the constructor accept the same strings.

`format: date` is asserted when `oo-core` compiles a schema. It does not reject year `0000`: `0000-02-29` can pass the format keyword and still fail `CivilDate`. `retryable` must match `ErrorCode` inside the Rust deserializer. That pair is not a JSON Schema constraint. A negative decimal scale is valid when the logical or physical type allows it. It is not a JSON Schema numeric bound. The `HttpsUrl` pattern rejects userinfo, a query, and a fragment. The Rust parser also rejects a path segment that literally contains `password=`, `token=`, `secret=`, `api_key=`, or `access_key=`. That list is not a general secret or PII detector. HTTP `Retry-After` is not a field of the error body.

## Versions

`config/v1`, `oos/v1`, `batch/v1`, `api/v1`, `type-policy/v1`. A missing or unknown version is one diagnostic (`version.missing` or `version.unsupported`). The parser does not guess an older shape.

YAML input is limited to 1 MiB, depth 32, and 50000 nodes. Aliases, anchors, tags, merge keys, directives, a second document, and a BOM are rejected. A `!` inside quotes is data. Diagnostics use `{contract}/{name}` with a JSON pointer, code, and severity. Example: `oo.config/v1/yaml.tag`.

Secret values are `secret://` references. `tls: disable` is rejected and is not a schema enum. The only interpolation exception is one whole claim scalar `{{ user.attributes.<field> }}` on a role binding. `${`, `$(`, backticks, and other `{{` forms fail.

Bigint, numeric, position, and export sizes are decimal strings. Query cells are null, bool, or string. A JSON number in those positions fails.

`QUERY_SQL_MAX_BYTES` is 65536 UTF-8 bytes. The instructions bound is 16384 UTF-8 bytes. A query cell is 500 Unicode scalars. `maxLength` on those byte-bounded strings is only a scalar ceiling of the same number. `VerifiedExample.sql` stays a 16000-scalar bound.

## Batch and source cells

Batch metadata carries protocol, source, table, schema fingerprint, batch id, path, optional file mode, config version, type policy, hook digest, transaction id, sequence, and epoch. System columns are `_oo_op` (`int8`, 1 upsert, 2 delete, 3 fill), `_oo_pos` (`decimal128(38,0)`), `_oo_src_ts` (`timestamp[us,UTC]`), and `_oo_unavailable` (`list<int16>`). A business column named `_oo_op` collides with a reserved name. The validator rejects the batch and tells the caller to rename the column before batch creation. The message may suggest `src__oo_op`. It does not rename, drop, or rewrite the input. Unknown `op` fails. CDC, incremental, and file upsert require a primary key that names business columns. Full may have an empty key. A snapshot uses `txn_sequence` 0 and puts the snapshot id in `txn_id`. Null and unavailable are different states.

Source cells are still not a PostgreSQL or MySQL driver. `fixtures/pg-mysql/cases.json` records independent decimal text. Asia/Shanghai `2026-10-01 09:12:03` is Unix `1790817123` (`date -u -r` prints `2026-10-01 01:12:03 UTC`). America/Chicago `2026-10-01 09:12:03` is CDT, Unix `1790863923`. Those instants come from the pinned IANA data, not from a fixed UTC+8 offset. America/New_York `2026-03-08 02:00:00` through `02:59:59` is a gap. `2026-11-01 01:00:00` through `01:59:59` is an overlap and both instants are returned. GB18030 and GBK bytes fail as `encoding_not_in_sp00_decoder` with a length and masked hex. `encoding_rs` 0.8.41 is a version pin so the resolver cannot select 0.8.42. `oo-core` does not call it and it is not a GB18030 decoder. A real driver dual path remains SP03.

## What a schema match does not prove

OpenAPI here has no paths. Matching `openapi-v1.json` does not prove HTTP status, idempotency storage, or an SDK. `ErrorCode::http_status` is metadata for the envelope. AT-CORE-03 as a full HTTP, Rust, and TypeScript SDK waits for SP09. Arrow series-60 spelling is checked for precision, scale, and list depth, then formatted by hand. `arrow-schema` Display was not executed, so that spelling is not claimed to match it. PositionCodec and real connectors are SP03.

Overlay merge (`merge_effective`) is a pure JSON rule: keyed lists merge by `name`, `id`, `code`, `term`, `metric`, `dataset`, `entity`, or `item`. Curated wins field by field. `status: rejected` removes the item and keeps the evidence. An empty curated keyed list does not clear generated items. A pin is stale only when it covers a path the curator actually overrode and the fingerprints differ. Coverage is the pin path or a descendant at a `/` boundary. `~` and `/` in a key are escaped as `~0` and `~1`. A real override with no covering pin is listed in `missing_pins` and the curated value stays. An unrelated pin is not stale. This crate does not hash upstream facts, and a successful merge is not AT-OOS-01. `missing_pins` is the cross-field result a later builder can require a review for.
