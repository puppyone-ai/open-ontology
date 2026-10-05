# Delimited reader `oo.source.delimited/v1`

This is a pull over any `std::io::Read`. The caller passes an explicit CSV or TSV profile and the SHA-256 of the whole object. The reader projects bool, int64, UTF-8 text, and exact decimal into an Arrow series 60 `RecordBatch`. It also returns stable source-row and field lineage.

`DelimitedComplete` is the only completion proof. A `DelimitedBatch` is not that proof. `DelimitedItem::Exhausted` is not that proof and has no boolean EOF field. `admits_import`, `published`, `arrow_ipc`, and `executes_native` stay false. A finished parse is not an import, not a publication, and not a PostgreSQL `RunComplete`.

The dialect is an argument. The reader does not choose a separator by looking at the file. It does not infer a column type or a semantic class. It does not open a database, write Arrow IPC, or execute a native payload.

## Dialect

`DelimitedDialect::try_from_separator` accepts comma (`csv`) and tab (`tsv`). Every other byte, including `;`, `|`, space, `"`, CR, and LF, is `ambiguous_dialect`. The quote character is `"`. The record ending is LF or CRLF. A quote inside a quoted field is written as `""`. A backslash is ordinary text.

The parser is a field state machine over the bytes already pulled into one fixed window. A physical line break is not a record boundary when it sits inside quotes. The last record may end at EOF with no trailing newline. A trailing newline does not create an extra record. A blank line is one empty field. Spaces are data. The reader does not skip them.

Outside quotes, a CR that is not part of CRLF is `bare_cr`. A quote that is not the first byte of a field, or any byte left between a closing quote and the separator or record ending, is `malformed_quote`. EOF inside quotes is `malformed_quote`. Bytes `0x00..=0x08`, `0x0B`, `0x0C`, `0x0E..=0x1F`, and `0x7F` are `control_char`. TAB, LF, and CR are not in that set. TAB is the TSV separator and is ordinary text in CSV.

Each finished field is decoded as strict UTF-8. Invalid UTF-8 is `encoding` / `utf8`. The error value stores a static fact. Display and Debug do not include cell bytes, the OS error string, or the profile text that failed.

## BOM and header

The UTF-8 BOM is the three bytes `EF BB BF`, examined only at the start of the stream. There is no sniff mode.

| Policy | Leading BOM | No BOM |
|---|---|---|
| `refuse` | `bom_refused` | read the first field |
| `strip` | consume the three bytes | read the first field |
| `require` | consume the three bytes | `bom_required` |

BOM bytes are part of the object hash. After `strip` or `require`, the first record starts at absolute offset 3. The BOM does not increment the line counter. A BOM later in the file is ordinary UTF-8 text (`U+FEFF`).

`header=absent` treats every record as data. `header=exact` requires the first record's decoded fields to equal the profile names in order. Header fields are not typed and are not passed through the null policy. A quoted header name is the decoded text. The header is not a data row and does not enter the batch. Width is checked before name equality (`width_mismatch`, then `header_mismatch`). An exact header that never arrives, including an empty object, is `header_required`. That check happens before the hash comparison.

An absent-header empty object covers the profile vacuously, the same way an empty JSONL object can complete. Zero data rows after a matched header can complete. Both still require a matching full-object hash.

## Nulls and types

Names and the profile id are exact UTF-8 labels: non-empty, at most 256 bytes, no ASCII controls, no leading or trailing whitespace. An internal space is allowed. Duplicate names and an empty column list are refused.

`NullPolicy::Empty`: an unquoted empty field is null. A quoted empty field is an empty string.

`NullPolicy::Token`: an unquoted field whose exact text is the token is null. A quoted token is literal text. An unquoted empty field is an empty string, not null. The token is 1 to 32 bytes, printable ASCII `0x21..=0x7E`, and it cannot contain `"` or the dialect separator. `\N` is a legal token. A comma token is legal on TSV and refused on CSV.

Null on a non-nullable column is `null_not_allowed`. An empty string on `bool`, `int64`, or `decimal` is `empty_field`. On `utf8`, a present empty string is a value with field presence `Empty`, which is distinct from null.

| Type | Accepted text | Refusal |
|---|---|---|
| `utf8` | the decoded field | none beyond UTF-8 and controls |
| `bool` | `true` or `false` | `TRUE`, `yes`, `1`, any other text (`not_bool`) |
| `int64` | optional `-`, then digits, no leading zero except `0` or `-0`, value in `i64` | `+`, `.`, exponent, space, leading zero (`not_int64`); outside `i64` (`int64_overflow`) |
| `decimal(p,s)` | optional `-`, digits, optional fraction of at least one digit, no leading zero on the integer part except a single `0` | `+`, exponent, space, trailing dot (`not_decimal`) |

There is no `f64` and no half-up rounding. `int64` and `decimal` both call the public `oo_core::ExactDecimal` API after the lexical check above. `parse_sql` trims and accepts a leading `+`, so this reader rejects space and `+` before that call. `-0` and `-0.00` collapse to a non-negative zero inside `ExactDecimal`. `i64::MIN` is converted through `i128` and then `i64`.

A decimal is then `rescale_exact` onto the profile scale. A larger scale only appends zeros (`1.2` into scale 2 is coefficient `120`). A smaller scale only strips trailing zeros (`1200` into scale `-2` is coefficient `12`). Any other change is `inexact_decimal`. `fits_decimal128` failure is `decimal_storage`. A coefficient longer than the profile precision after rescale is `decimal_precision`. A coefficient the exact parser rejects for budget is `number_budget`.

Profile precision is `1..=38`. Profile scale is `-38..=38`. A positive scale greater than precision is refused at profile construction. A negative scale is allowed. The Arrow value is `Decimal128Array` with that precision and scale. Field metadata is `oo.delimited.protocol`, `oo.delimited.logical`, and for decimal `oo.delimited.precision` and `oo.delimited.scale`.

## Identity

Profile preimage, UTF-8, then SHA-256:

```text
oo-delimited-profile/v1
id=<profile id>
dialect=csv|tsv
bom=refuse|strip|require
header=absent|exact
null=empty
quote=double
record=lf-or-crlf
escape=double-quote
column=<name>\t<bool|int64|utf8|decimal>\t<0|1>\t<precision|->\t<scale|->
```

When the null policy is a token, the null lines are:

```text
null=token
null_token=<token>
```

One `column` line per profile column, in order. Non-decimal precision and scale are `-`. Config identity is SHA-256 of:

```text
oo-delimited-config-identity/v1
<profile preimage>
expected_sha256=<64 lowercase hex>
```

Budgets are not in either preimage. The same profile and object hash keep the same `config_identity` under two budgets.

## Pull

`DelimitedBudget::try_new` takes every cap explicitly. `queue_batches` is 1 and is not a parameter.

| Field | Bound |
|---|---|
| `batch_rows` | `1..=65536` |
| `batch_bytes` | `1..=16777216` |
| `max_record_bytes` | `3..=1048576` |
| `max_field_bytes` | `1..=max_record_bytes`, and at most 1048576 |
| `max_total_bytes` | at least `max_record_bytes`, at most 5 GiB |
| `max_rows` | `1..=16777216` |
| `max_columns` | `1..=256`, and at least the profile width |
| `max_fields` | `1..=256`, and at least the profile width |
| `deadline` | checked at open and before each `read` |

The record floor is 3 so the BOM lookahead fits in the one window allocated at `open`. `open` does not read. The window length is exactly `max_record_bytes` and is not grown. A longer record is `max_record`. Decoded field bytes are capped by `max_field_bytes` before each extra byte is stored. `max_fields` is checked before the next field body is read. A finished record whose width differs from the profile, and whose field count is still within `max_fields`, is `width_mismatch`.

`batch_bytes` counts raw record bytes, including the record ending. Header bytes are hashed and limited by the record and field caps. They do not count as data rows or `batch_bytes`. A data row that does not fit a non-empty batch is left for the next pull. A data row that does not fit an empty batch is `batch_bytes`. `max_rows` stops the same way. Rows already parsed and not yet returned are dropped when a later record fails. Batches already returned stay untrusted until `DelimitedComplete`.

Input bytes are SHA-256'd as they are read, before they are parsed. The digest is compared with `expected_sha256` only when `Read` returns EOF at a record boundary. A hash mismatch is `hash_mismatch`. If a batch is pending at that moment, that batch is returned once and the next call returns `hash_mismatch`. Past `max_total_bytes`, one extra byte is probed and not hashed. That path is `max_total`. An I/O failure is `io` / `read`. The OS message is dropped. Interrupted reads are retried.

Lineage is data rows only. `row_index` is 0-based among data rows. `line` is the 1-based physical line of the record's first byte. LF increments the line, including LF inside quotes. `byte_offset` is absolute from the first byte of the `Read`, including a stripped BOM. Each field records the profile name, the line and offset of its first raw byte (the opening quote when the field is quoted), and `Value`, `Null`, or `Empty`. Every emitted row has one field entry per profile column.

`next_batch` returns `Exhausted` only after a clean EOF, a covered profile, and a matching hash. `finish` mints `DelimitedComplete` only after that value was delivered, the cancel flag is clear, and the stored digest equals the expected digest. `finish` before that returns `not_exhausted`. `cancel` and `Drop` release the owned reader, zero the record window, and do not mint a token.

## Blocking read

Cancel and the deadline are checked before each `read`, not during it. A `Read` implementation can block inside one `read` after those checks have passed. This reader does not interrupt that call and does not claim a cancel-during-read proof. After the blocked `read` returns, the next check observes the flag. `finish` then refuses `DelimitedComplete`.

## Out of scope

GB18030, other charsets, delimiter sniffing, DuckDB, chardet, Arrow IPC, worker-process isolation, file hosting, object stores, the loader, replace/append/upsert, and publication stay open. This reader is the `Read` source those later adapters can wrap. It does not close the full CSV/TSV format checks in the product spec.
