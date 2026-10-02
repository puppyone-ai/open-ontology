# PostgreSQL type conversion `oo.source.pgsql.types/v1`

This protocol decodes PostgreSQL binary values. It does not replace the frozen catalog protocol `oo.source.pgsql/v1`. Catalog `type_support.converted` stays false. Catalog `type_conversion` stays `unsupported`. `connector_capabilities` stays unsupported, including `type_conversion`. `numeric_parts` stays the unsigned inspection decoder. A caller `converted=true` flag is ignored.

`PgSession::read_typed` is the production entry. It uses the same private-CA `verify-full` session, startup allow-list, and repeatable-read read-only transaction as catalog inspection, then rolls back. It does not `SET ROLE`, run user output functions, or execute SQL built from values.

## Signed numeric typmod

`pg-numeric-typmod/v1` decodes the PostgreSQL signed 11-bit scale: `((typmod - 4) & 0x7ff) ^ 0x400) - 0x400`. `numeric(10,-2)` is precision 10 and scale -2. The same typmod still reports catalog v1 scale 2046 through `numeric_parts`. Both numbers are kept on `ColumnMapping`. When the low 11 bits are `0..=1023`, the two scales agree.

## Column mapping

`ColumnMapping` carries `source_type`, `arrow_type`, `pg_type`, `precision`, `scale`, `timezone`, `nullable`, and `type_policy_version` (`type-policy/v1`). `arrow_type` is the workspace series-60 spelling from `oo-core`, such as `decimal128(10,-2)`, `list<int32>`, and `timestamp[us,UTC]`. It is not arrow-schema `Display`.

The plan is built from `pg_type` and `pg_proc` rows loaded inside the snapshot. The send function must be `pg_catalog` and, for a builtin, the expected name (`boolsend`, `numeric_send`, `jsonb_send`, and the rest of the fixed list). A user type whose send function is named `boolsend` but whose oid is not `bool` stays unsupported.

| Input | Result |
|---|---|
| Builtin bool, int2, int4, int8, float4, float8, text, varchar, bpchar, bytea, uuid, json, jsonb, date, time, timestamp, timestamptz, interval, numeric | Lossless binary decode |
| Domain | Decode as the base, at most 8 steps, no cycles. The catalog oid stays the domain oid. PostgreSQL's result descriptor reports the base type, and that oid is accepted. Any other oid fails the read. |
| Enum | `enum_send` label, checked against catalog labels |
| Array of a lossless element | Lower bounds and lengths are canonical. The Arrow value is one flat list in storage order. The header element oid must match the element plan oid, or the base oid when the element is a domain. At most 6 dimensions and 100000 elements. |
| money | `money_locale`. No locale parse and no numeric cast |
| timetz, composite, range, multirange, pseudo, other user types | Explicit unsupported reason |
| Missing `SELECT` | `select_not_granted` |

Numeric Arrow choice, after the signed typmod:

| Declared shape | Arrow |
|---|---|
| Precision 1..=38 and scale fits series-60 rules | `decimal128` |
| Precision 39..=76 and scale fits | `decimal256` |
| Positive scale greater than precision, scale outside `i8`, precision above 76, or unconstrained numeric | Exact decimal `utf8` plus `oo.pg.decimal=exact` and precision/scale metadata |

Unconstrained numeric uses logical precision 0 and scale `i16::MIN`. A value that does not fit the declared precision, or that cannot be rescaled exactly, rejects `numeric_precision` or `numeric_scale`. There is no truncation, rounding, or saturating cast. Numeric NaN, +infinity, and -infinity reject. Numeric negative zero canonicalizes to unsigned zero. Float negative zero keeps its sign bit in the Arrow `float64` value; the canonical float class collapses both signed zeros.

`json` is exact stored text and may contain duplicate keys. `jsonb` is version byte 1 plus UTF-8 text, parsed and rendered as canonical JSON. The two logical types differ, so their canonical hashes differ. JSON null is a value, not SQL NULL.

Date, time, timestamp, and timestamptz are decoded from the binary counts. DateStyle, IntervalStyle, and session TimeZone are not applied. `timestamptz` is mapped with timezone `UTC`. Infinity and values outside the canonical range reject. Interval is a canonical struct of months, days, then microseconds, and an Arrow `list<int64>` in that order.

## Rows and rejects

`PgTypePolicy` has no mode that drops a row or rewrites a value. The default keeps the row.

SQL NULL is `CellPresence::Null` and an Arrow null. It is not a reject. A decode failure is `CellPresence::Unavailable`, an Arrow null, and a masked reject. The reject records source, schema, table, column, attnum, path, row index, key digest, reason, wire `value_len`, `run_generation`, input digest, and type-policy version. It does not record a sample or a driver exception.

An unsupported non-key column is omitted from the `SELECT` and recorded on every row with `value_len` 0. An unsupported key, a null or unavailable key, or a null in a non-nullable column fails the whole read. `FailRead` fails before fetch when any column is unsupported. Text-format driver values fail the whole read. A result oid is accepted only when it is the catalog column oid or, for a domain, the base `decode_oid`. Fetching `max_rows + 1` fails the budget and returns no partial batch.

The canonical hash is `type-policy/v1` over the lossless fields only. It is not a hash of Arrow buffers. `plans` still lists every source column. Full reject persistence belongs to the later Loader and is not done here.

## What this protocol does not do

`TypedBatch::record_batch` builds an Arrow 60 `RecordBatch` with `arrow-array`, `arrow-buffer`, and `arrow-schema` `60.0.0` (`default-features = false`). Field types follow the series-60 physical plan, including `decimal128`, `decimal256`, `timestamp[us,UTC]`, and flat lists. A field is nullable when the catalog column is nullable or any cell is an Arrow null, including a masked reject in a catalog `NOT NULL` column. SQL NULL in a non-nullable column still fails the read before a batch exists. The canonical hash is still the type-policy frame, not a hash of those buffers. `pg_arrow_crate` is supported for that builder. MySQL unsigned, PositionCodec, CDC, the bounded reader, Loader reject persistence, and user-defined output stay unsupported. This slice does not claim the reader or the Loader.
