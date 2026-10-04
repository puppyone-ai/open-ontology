# copy-encoder/v1

`copy_encoder` turns one `TypedBatch` plus an approved COPY column list into PostgreSQL binary COPY bytes. Protocol `copy-encoder/v1`. The bytes are the `FORMAT binary` representation: the 19-byte signature header, tuple bodies, and the `0xffff` trailer. This crate does not open a connection, create a table, take an advisory lock, write a receipt, or publish a read. It is not the Loader. Full reader admission and full apply stay gated.

The module re-exports the existing source builders (`assemble_batch`, `map_column`, and the typed cell types) so a caller of this crate can name that contract without a new dependency. Those re-exports do not add a codec.

## Pull

`CopyEncoder::open` borrows the batch and the caller-supplied system positions. `pull` returns `Header`, then one or more `Rows` pieces, then `Trailer`, then `Done`. A later `pull` after `Done` returns `Done` again. There is no background queue and no thread. The caller passes the next batch; this encoder does not concatenate batches.

Each `Rows` piece holds at most `max_rows_per_chunk` tuples and at most `max_chunk_bytes` bytes. A row that fits the row budget but not the remaining chunk ends the pull. The next pull starts at that row. A row that cannot fit an empty chunk is `chunk_budget`. Value, row, and chunk limits are applied before the chunk buffer is allocated. Numeric digit groups are computed from the coefficient; they are not stored in a side buffer.

`max_value_bytes`, `max_row_bytes`, or `max_rows_per_chunk` of zero, or `max_chunk_bytes` below 19, is `closed_budget`. A payload above `max_value_bytes` or above `i32::MAX` is `value_budget`. A tuple above `max_row_bytes` is `row_budget`. An encode error poisons the encoder. Later pulls return that same error. Pieces already returned are not retracted; the caller aborts COPY.

## Columns

Index 0 of the approved list is `CopyColumn::SystemPosition`. It is not a `TypedBatch` field and not a user `attnum`. The caller passes one `ExactDecimal` per row. The value must be non-negative, scale 0, and at most 38 digits (`fits_position`). It is encoded as PostgreSQL `numeric`. A list that does not start with that column is `system_position_order`. A second system column is `duplicate_system_position`. A list with no user column is `zero_user_columns`.

User columns follow in the approved list order. That order is the COPY column order. It is not `attnum` order and it is not `TypedBatch::fields` order. Each `attnum` must appear once and must select a lossless plan. The cell slot is the lossless field index, skipping plans that are not lossless. An unknown `attnum` is `unknown_column`. A plan that is not lossless is `unsupported_column` with the plan's reason. The field count must fit in `i16`.

An empty batch (zero rows, empty position slice, at least one user column) is a valid empty COPY: header, trailer, and no tuple body.

## NULL, unavailable, and masked values

`CellPresence::Null` on a nullable column is the COPY NULL marker (`int32` `-1`). NULL on a column the plan marks not nullable is `null_not_allowed`. `CellPresence::Unavailable` is `unavailable`. The detail is the reject reason, or `unavailable` when the cell has no reject. The only policy is `UnavailablePolicy::Refuse`. An unavailable or masked value is not written as SQL NULL and is not dropped. Unmapped source rejects stay on `TypedBatch::rejects`; this encoder does not persist them.

A cell that mixes a value with a reject, a null with a value, or a logical value that disagrees with its Arrow scalar is `inconsistent_cell` or `value_mismatch`. Nothing in those cases is coerced to NULL.

## Types this codec writes

The encoder follows the approved source logical value and the Arrow scalar already on the cell. It does not widen the source type system.

| COPY payload | Source |
|---|---|
| bool `0`/`1` | `bool` |
| `int2` / `int4` / `int8` big-endian | `int2` / `int4` / `int8` |
| `float4` big-endian | `float4` only when the stored `f64` bits are an exact finite `f32` widening, including signed zero |
| `float8` big-endian | `float8` Arrow bits, including signed zero and a NaN payload |
| exact UTF-8 | `text`, `varchar`, `bpchar`, `json` |
| raw bytes | `bytea`; `uuid` when the payload is 16 bytes |
| `int32` days since 2000-01-01 | `date` |
| `int64` microseconds since midnight | `time` |
| `int64` microseconds since 2000-01-01 | `timestamp`, `timestamptz` |
| numeric base-10000 digits | `numeric` whose scale is ≥ 0, including `numeric(38,0)` positions and decimal128 / decimal256 / exact decimal text |

`bpchar` and `varchar` are emitted as stored. They are not padded and not truncated. A character limit on the plan (`typmod - 4`) refuses a longer Unicode scalar count with `char_length`. A `text` NUL is `text_nul`. `json` is the exact stored text, not a rewritten document. `float4` NaN is `float4_nan`. A `float4` value that does not round-trip through `f32` is `float_precision`. Canonical `FloatClass::Zero` collapses the sign; the Arrow bits are what COPY sends, and a class that disagrees with those bits is `value_mismatch`.

Numeric scale below zero is `numeric_scale` and is not rewritten to scale 0. A value that disagrees with the declared precision, the declared scale, or the Arrow unscaled coefficient is `value_mismatch`. Digit groups that do not fit the binary header are `numeric_range`.

A domain column encodes as its base `decode_oid` when that base is in the table above. Domain check constraints are not applied here.

## Refused on purpose

These are typed refusals, not a claim that the codec covers PostgreSQL:

- `jsonb` (`type_not_encoded`, detail `jsonb`). The cell holds canonical JSON, not the version-byte wire payload.
- `interval` (`interval`)
- arrays (`array`) and enums (`enum`)
- any other oid (`oid`), including money, timetz, composite, range, and multirange when a plan is not lossless (`unsupported_column` instead)
- negative numeric scale (`numeric_scale`)

`arrow_dimensions` calls `TypedBatch::record_batch` and returns the series-60 row and column counts. It does not hash Arrow buffers and it does not publish a read.

## Fixture proof

The ignored integration test speaks the PostgreSQL 17.10 wire protocol, sends each pulled piece as its own `CopyData`, and lets the server decode the rows. The fixture statements use literal identifiers only. The COPY runs inside a transaction that rolls back; `to_regclass` of the fixture table is null afterward. That write is test administration. It is not a production COPY sink and it does not set a target advisory fence.
