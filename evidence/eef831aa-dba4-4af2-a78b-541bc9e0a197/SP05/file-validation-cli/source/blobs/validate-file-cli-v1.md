# validate-file command

`validate-file --request <path>` reads one `oo.pipeline.validate-file/v1` document and, only after `finish` returns a receipt, writes one `oo.pipeline.validate-file-result/v1` document to stdout. `validate-file --help` writes the usage text to stdout and exits 0.

The command checks a local CSV, TSV, or JSONL file against an explicit profile. It does not infer a dialect, a column type, or a semantic class from the bytes. It does not import rows, publish a table, open a database, write Arrow IPC, or execute a native payload. `admits_import` and `published` on a success document are the flags from the finish receipt. This command refuses to print a success document when either flag is true.

The delimited and JSONL readers are called through their public constructors (`DelimitedDialect::try_from_separator`, `DelimitedColumn::try_new`, `DelimitedProfile::try_new`, `DelimitedBudget::try_new`, `DelimitedReader::open`, and the JSONL equivalents). This command does not accept those libraries. A review of the command still has to check that integration. It does not close the reader review.

## Usage

The process accepts exactly one of these argument lists:

| Arguments | Result |
|---|---|
| `--help` | usage text on stdout, exit 0 |
| `--request <path>` | the request file at `path` |

Any other list, including an unknown flag, `-h`, `--help` with another argument, or `--request` without a path, is usage. Usage writes one error document to stderr and writes nothing to stdout. Exit 2.

The usage text names the request protocol, the 1048576 byte cap, CSV, TSV, and JSONL, and states that the command does not infer a dialect, does not import, does not publish, does not open a database, does not write Arrow IPC, and does not interrupt a blocking read.

## Request

The request path is opened as a regular file. Its length is taken from metadata before a body buffer is filled. A length above 1048576 bytes is `request_bytes` and the body is not read into a buffer of that size. The read itself is also capped at 1048577 bytes, so a short metadata lie cannot allocate the rest of a growing file. A request of exactly 1048576 bytes is parsed. The bytes must be UTF-8.

`oo_core::parse_json_value` then rejects duplicate keys, excess depth, excess nodes, and trailing bytes. Every object below has exactly the listed keys. JSON null is not a string, a boolean, or an integer. Integers have no fraction. `expected_sha256` is 64 lowercase hex digits. An uppercase digit is `expected_sha256`.

`protocol` is `oo.pipeline.validate-file/v1`. Any other string is `version`.

`format` is `csv`, `tsv`, or `jsonl`. Any other string is `format`. The file bytes are not consulted.

`input_path` is the local file to validate. An empty path or a path containing NUL is `input_path` and the path is not opened. The command does not fetch HTTP, does not authenticate a public caller, and does not claim an authorization decision. The caller is trusted to name a local file. The path is not jailed and it is not echoed in a diagnostic.

`cancel_before_read` is a boolean. `true` asks the command to set the reader cancel flag after `open` and before `next_batch`. It does not spawn a thread.

`profile` and `budget` depend on `format`.

### CSV and TSV profile

Keys: `id`, `dialect`, `bom`, `header`, `null`, `columns`.

`dialect` must be `csv` when `format` is `csv`, and `tsv` when `format` is `tsv`. The command passes `b','` or `b'\t'` to `DelimitedDialect::try_from_separator`. A disagreement is `dialect`. No other separator is sniffed.

`bom` is `refuse`, `strip`, or `require`. `header` is `absent` or `exact`.

`null` is `{"policy":"empty"}` or `{"policy":"token","token":"<text>"}`. The token is checked by `DelimitedProfile::try_new`.

`columns` is a list. Each non-decimal column has keys `name`, `type`, `nullable`. `type` is `utf8`, `bool`, or `int64`. A decimal column has those keys plus `precision` and `scale`, and `type` is `decimal`. `precision` and `scale` on a non-decimal column are `unknown_field`. `float` and `json_text` are `type`. Each column is built with `DelimitedColumn::try_new`, then the profile with `DelimitedProfile::try_new`. Constructor facts (`column_name`, `decimal_precision`, `decimal_scale`, `profile_id`, `empty_selection`, `duplicate_column`, `null_token`, and the budget facts) are returned as config.

### JSONL profile

Keys: `id`, `columns`. A dialect, BOM, header, or null key is `unknown_field`.

Each column has keys `name`, `type`, `nullable`. `type` is `utf8`, `bool`, `int64`, or `json_text`. `decimal` is `type`. Columns are built with `JsonlColumn::try_new`, then `JsonlProfile::try_new`.

### Budget

CSV and TSV keys: `batch_rows`, `batch_bytes`, `max_record_bytes`, `max_field_bytes`, `max_total_bytes`, `max_rows`, `max_columns`, `max_fields`, `deadline_ms`.

JSONL uses the same keys except `max_field_bytes`. That key on a JSONL budget is `unknown_field`.

`deadline_ms` is 1 through 120000. The instant passed to `try_new` is that many milliseconds after the budget is built. The reader checks it before `open`'s read and before each later `read`. A value outside the range is `deadline` and `try_new` is not called. Other caps are passed through `DelimitedBudget::try_new` or `JsonlBudget::try_new`. `queue_batches` is not a request field. The library fixes it at 1.

There is no database, password, CA, or SQL field. An extra key such as `password` is `unknown_field`. The value is not copied into the diagnostic.

## Pull

The input path is opened with `File::open` after the request has been accepted. A missing file is outcome `read`, fact `open`. A path that opens and is not a regular file is `not_file`. The operating-system message is dropped. This happens before cancel is requested, so a missing file stays `read` when `cancel_before_read` is true.

`DelimitedReader::open` or `JsonlReader::open` receives that `File`, the constructed profile, the decoded SHA-256, and the constructed budget. The command then calls `next_batch` until `Exhausted` or an error. Each batch must have `resumable` false and closure flags false. Lineage is counted. Row payloads are not stored for stdout.

On `Exhausted`, the command calls `finish`. A hash mismatch, a parse error, a budget error, or a cancel returns no receipt. Batches already pulled are discarded. There is no fallback success.

`finish` must agree with the exhausted item on rows, batches, and object hash, and with the header on protocol, profile id, profile digest, and config identity. The folded row count must equal the receipt. A disagreement is `receipt` and is not a success document.

## Success document

Stdout is one JSON object, keys sorted, with a trailing newline. The fields are copied from the finish receipt and the reader header, plus the lineage fold:

| Field | Source |
|---|---|
| `protocol` | `oo.pipeline.validate-file-result/v1` |
| `source_protocol` | receipt protocol (`oo.source.delimited/v1` or `oo.source.jsonl/v1`) |
| `profile_version`, `lineage_version`, `receipt_version` | receipt |
| `profile_id`, `profile_digest`, `config_identity`, `object_sha256` | receipt, digests as 64 lowercase hex |
| `rows`, `columns`, `batches` | receipt |
| `admits_import`, `published` | receipt |
| `arrow_ipc`, `executes_native` | reader header |
| `types` | header column name, logical type, nullability; decimal adds `precision` and `scale` |
| `lineage` | fold below |

`lineage` keys: `rows`, `fields`, `value`, `null`, `empty`, `missing`, `first_line`, `last_line`, `first_byte_offset`, `last_byte_offset`. Counts are presence classes from the reader lineage. Delimited pulls leave `missing` at 0. JSONL pulls leave `empty` at 0. With no data rows every lineage number is 0. The object does not contain cell text, field offsets per row, the input path, or a credential.

## Errors

Stderr is one JSON object, keys sorted, with a trailing newline. Stdout is empty. The keys are `protocol` (`oo.pipeline.validate-file-error/v1`), `outcome`, and `fact`. `fact` is a static token. It is not a path, a cell, an operating-system message, or a request value.

| Exit | Outcome | When |
|---|---|---|
| 0 |  | success document on stdout |
| 2 | `usage` | arguments |
| 3 | `config` | request shape, version, format, profile, or budget |
| 4 | `read` | input open, input is not a regular file, or the result could not be written |
| 5 | `parse` | reader protocol or encoding, including `not_bool`, `not_int64`, `not_decimal`, `inexact_decimal`, `header_required`, `bom_refused` |
| 6 | `budget` | reader budget, including `max_rows` and `deadline` |
| 7 | `hash` | `hash_mismatch` |
| 8 | `cancel` | `cancelled` |

Reader config facts stay outcome `config`. Reader I/O facts stay outcome `read`.

## Blocking read

Cancel and the deadline follow the reader rule. They are checked before each `read`, not during it. A `Read` that blocks inside one call is not interrupted. This command does not add a worker, a timeout thread, or an isolation boundary around that call. After the call returns, the next check observes a cancel flag set before the pull.

## Out of scope

Import, publication, PostgreSQL, HTTP, Arrow IPC, worker isolation, dialect sniffing, GB18030, Parquet, Excel, and a public authentication claim stay out of this command. The CSV and TSV library review is not closed by shipping this binary.
