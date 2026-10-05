# JSONL bounded reader `oo.source.jsonl/v1`

This is a pull over any `std::io::Read`. The caller passes an immutable profile and the SHA-256 of the whole object. The reader does not infer a format, a delimiter, a column type, or a semantic class. It does not open a database, write Arrow IPC, sniff with DuckDB, or execute a native payload.

`JsonlComplete` is the only completion proof. A `JsonlBatch` is not that proof. `JsonlItem::Exhausted` is not that proof and has no boolean EOF field. `admits_import` and `published` are false on the header and on the token. A finished parse is not an import and not a publication.

PostgreSQL `TypedBatch` is not reused. Its public builder requires a PostgreSQL column plan. Decimal conversion in that path is private to the binary decoder and is not a JSON entry point, so this slice does not certify a decimal column.

## Profile

`JsonlProfile::try_new` takes an id and a non-empty column list. Names are exact UTF-8 labels: non-empty, at most 256 bytes, no ASCII controls, no leading or trailing whitespace. Duplicate names and an empty list are refused (`duplicate_column`, `empty_selection`). There is no second include list and no rename map. The JSON key, after string decoding, must equal the column name. Arrow field order is the profile order, not the object order.

| Type | JSON value | Refusal |
|---|---|---|
| `utf8` | one JSON string | objects, arrays, numbers, bools |
| `bool` | `true` or `false` | numbers, strings, objects, arrays |
| `int64` | a JSON number whose text is an optional `-` and digits, and which fits in `i64` | `.`, `e` / `E`, overflow, strings, bools, objects, arrays |
| `json_text` | any one JSON value except `null` | the original value bytes, including nested objects and arrays |

`null` is Arrow null when the column is nullable, including `json_text`. It is not the four characters `null`. On a non-nullable column it is `null_not_allowed`. A missing key is Arrow null only when the column is nullable (`missing_key` otherwise). Missing and JSON null stay distinct in field lineage.

An unknown key is `unknown_key`. A duplicate key is `duplicate_key`, including `\u` escapes that decode to the same key, and including duplicate keys inside a `json_text` value. No row is skipped. Nested objects and arrays on `utf8`, `bool`, or `int64` are `nested_refused`. They are not stringified.

`int64` uses `oo_core::ExactDecimal::parse_json_number` and then `i64`. `1`, `-0` (as integer 0), `i64::MIN`, and `i64::MAX` are accepted when the lexical form has no fraction and no exponent. `1.0`, `1.5`, and `1e2` are `not_int64`. A value outside `i64` is `int64_overflow`. There is no `f64` conversion. A coefficient the exact parser rejects for budget is `number_budget`. Leading zeros and a leading `+` are not JSON numbers (`invalid_number` or `type_mismatch` for `+`).

The profile has no PII flag, semantic class, or ontology tier. The reader does not add one.

## Identity

Profile preimage, UTF-8, then SHA-256:

```text
oo-jsonl-profile/v1
id=<profile id>
column=<name>\t<utf8|bool|int64|json_text>\t<0|1>
```

One `column` line per profile column, in order. Config identity is SHA-256 of:

```text
oo-jsonl-config-identity/v1
<profile preimage>
expected_sha256=<64 lowercase hex>
```

Budgets are not in either preimage.

## Pull

`JsonlBudget::try_new` requires:

| Field | Bound |
|---|---|
| `batch_rows` | `1..=65536` |
| `batch_bytes` | `1..=16777216` |
| `max_record_bytes` | `1..=1048576` |
| `max_total_bytes` | at least `max_record_bytes`, at most 5 GiB |
| `max_rows` | `1..=16777216` |
| `max_columns` | `1..=256`, and at least the profile width |
| `max_fields` | `1..=256`, and at least the profile width |
| `deadline` | checked at open and before each read and record |

`queue_batches` is 1. `open` checks the deadline, the column cap, and the field cap before it allocates the record window. It does not read. The window length is `max_record_bytes`. A longer record is `max_record` and the window is not grown. `read_to_end` and `read_line` are not used.

Input bytes are SHA-256'd as they are read, before they are parsed. The digest is compared with `expected_sha256` only when `Read` returns EOF at a record boundary. A matching prefix is not success. A hash mismatch is `hash_mismatch` and does not mint a token. Bytes already delivered in a batch stay untrusted. A truncated value is `truncated`, not a successful hash. Past `max_total_bytes`, one extra byte is probed and not hashed; that path is `max_total`, not EOF.

A record is one JSON object. Whitespace (`space`, tab, CR, LF) around objects is allowed, so one object may span lines. Top-level arrays and scalars are `not_object`. Raw controls inside strings are `control_char`. Invalid UTF-8 is `utf8`. Lone UTF-16 surrogates are `utf16_surrogate`. Nesting inside `json_text` past 32 is `depth`. An array past 100000 elements is `array_elements`. Each object, including a nested one, may have at most `max_fields` keys.

`batch_bytes` counts raw record bytes from `{` through `}`. A row that does not fit a non-empty batch is left unread for the next pull. A row that does not fit an empty batch is `batch_bytes`. `max_rows` stops the same way.

The Arrow batch uses series 60 `StringArray`, `BooleanArray`, and `Int64Array`. `json_text` is UTF-8 with field metadata `oo.jsonl.logical=json_text` and `oo.jsonl.encoding=raw-json-utf8`. Nullability is the profile flag. `resumable` is false. `JsonlItem::Batch` holds a `Box<JsonlBatch>` so the exhaustion variant is not the size of a record batch.

Lineage on each row is the 1-based line and absolute byte offset of `{`, plus one field entry per profile column. A present or null field records the key's opening quote. A missing nullable field records the record start and `Missing`.

`next_batch` returns `Exhausted` only after a clean EOF and a matching hash. `finish` mints `JsonlComplete` only after that `Exhausted` value was delivered, the cancel flag is clear, and the stored digest equals the expected digest. `finish` before that returns `not_exhausted`. `cancel` and `Drop` release the owned reader and do not mint a token. The record window is zeroed on drop. There are no database or model credentials.

A stalled `read` is not interrupted. The deadline is checked before each `read`, not during it.

## Out of scope

CSV, TSV, Parquet, Excel, DuckDB, Arrow IPC workers, file-hosting adapters, object-store clients, GB18030, replace/append/upsert, and publication stay open. This reader is the `Read` source those later adapters can wrap.
