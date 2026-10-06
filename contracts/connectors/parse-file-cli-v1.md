# parse-file command

`parse-file` reads one `oo.connectors.parse-file/v1` document from stdin. Only after `ParentPull::next_batch` returns `Ok(None)` and `ParentPull::finish` returns `ParentParseComplete` does it write one `oo.connectors.parse-file-result/v1` document to stdout. `parse-file --help` writes the usage text to stdout and exits 0.

The command admits one local CSV, TSV, or JSONL file by an explicit profile, an expected object SHA-256, and the absolute path of a compiled `oo-file-parser-worker`. It hashes that executable and calls `ParentConfig::try_new`, `ChildRequest::try_new`, and `ParentConfig::start`. Batches are pulled one at a time. Each record batch is dropped after its lineage counters are added. The summary has counts and digests. It has no cell text, no raw row, and no input path.

This command does not call `DelimitedReader` or `JsonlReader`. It does not build a worker frame, an Arrow IPC stream, or a reader receipt of its own. `ParentParseComplete` is the only success source. `admits_import`, `published`, and `pg_run_complete` are that receipt's methods. A true value is not printed as success. `kernel_restriction` is the public `KERNEL_RESTRICTION` constant. The only implemented value is `unsupported`. That string is not a containment certificate. This command does not add an escape campaign, a `setsid` child, seccomp, landlock, or an rlimit.

SP05-W02, AT-FILE-02, and the phase ledger stay open. The command does not admit an import, a publication, or a PostgreSQL run.

## Usage

The process accepts exactly one of these argument lists:

| Arguments | Result |
|---|---|
| none | one request document on stdin |
| `--help` | usage text on stdout, exit 0 |

Any other list, including an unknown flag, `-h`, `--request`, or `--help` with another argument, is usage. Usage writes one error document to stderr and writes nothing to stdout. Exit 2.

The usage text names `oo.connectors.parse-file/v1`, the 1048576 byte cap, CSV, TSV, and JSONL. It states that the command does not infer a dialect, does not import, does not publish, does not open a database, prints no raw rows, and returns `ParentParseComplete` only after a clean EOF. It names `parent_deadline_ms`, says kernel restriction is `unsupported`, and lists exits 0, 2, 3, 4, 5, 6, and 7.

## Request

Stdin is read in 8192 byte chunks. The command stops at 1048577 bytes. A document longer than 1048576 bytes is `request_bytes` and is not parsed. The retained bytes must be UTF-8. Empty input is `request_json`.

`oo_core::parse_json_value` then rejects duplicate keys, excess depth, excess nodes, and trailing non-whitespace. Trailing spaces, tabs, CR, and LF are accepted. Every object below has exactly the listed keys. JSON null is not a string, a boolean, or an integer. Integers have no fraction and no exponent. `sha256` fields are 64 lowercase hex digits. An uppercase digit is the field's own fact.

These keys are rejected anywhere in the tree before other unknown-key checks. The fact is `credentials`. The key name and the value are not written to stderr: `access_key`, `api_key`, `authorization`, `connection_string`, `credential`, `credentials`, `dsn`, `passwd`, `password`, `private_key`, `secret`, `secret_key`, `token`.

`protocol` is `oo.connectors.parse-file/v1`. Any other string is `version`.

The top-level keys are exactly `protocol`, `executable`, `input`, `parent_deadline_ms`, `format`, `profile`, and `budget`.

### Executable

Keys: `path`, `sha256`.

`path` is the compiled worker. `ParentConfig::try_new` applies its absolute regular-file rule. A refusal is `worker_path` and the file is not hashed. There is no shell and no argument vector.

The command then streams SHA-256 of that path in 8192 byte chunks. The read is capped at 2147483648 bytes. A larger regular file is `executable_bytes` and is not passed to `start`. The hash clock is 120000 ms, measured from the start of this read. It is not `parent_deadline_ms`. The parent deadline is not extended to cover this hash, and a parent deadline of 1 ms still allows the hash clock to finish. Exceeding the hash clock is `deadline` and does not call `start`. A length change during the read is `executable_io`.

A digest that differs from `sha256` is outcome `hash`, fact `executable_sha256`. `start` is not called. `reaped` is false.

The check and the later `start` are not one atomic snapshot. This command does not prove the executable stays unchanged.

### Input

Keys: `path`, `sha256`.

`path` is the caller-admitted local file passed to `ChildRequest::try_new`. The command does not open it itself and does not compare its bytes before `start`. A hash that differs from the file still starts the worker. That call cannot return `ParentParseComplete`. `sha256` is 64 lowercase hex digits. A bad spelling is `input_sha256` and does not start the worker.

`ChildRequest` path failures stay on the parent facts: `path_form`, `input_io`, `not_regular`, `input_too_large`, `input_changed`, `profile_budget`.

### Clocks

`parent_deadline_ms` is an integer in `1..=120000`. Any other integer is `deadline`. `ParentConfig::try_new` is not called. The executable is not opened.

`budget.deadline_ms` is the worker's between-read clock, an integer in `1..=3600000`. Any other integer is `worker_deadline`. `PullBudget` is not called. This clock is not the parent deadline. The command does not add them together.

### Format and profile

`format` is `csv`, `tsv`, or `jsonl`. Any other string is `format`. The file bytes are not consulted.

CSV and TSV profile keys: `id`, `dialect`, `bom`, `header`, `null`, `columns`.

`dialect` is `csv` when `format` is `csv`, and `tsv` when `format` is `tsv`. The command passes `b','` or `b'\t'` to `DelimitedDialect::try_from_separator`. A disagreement is `dialect`.

`bom` is `refuse`, `strip`, or `require`. `header` is `absent` or `exact`.

`null` is `{"policy":"empty"}` or `{"policy":"token","text":"<marker>"}`. The marker key is `text` because `token` is a credential key. The public constructor checks the marker.

`columns` is a list. Each non-decimal column has keys `name`, `type`, `nullable`. `type` is `utf8`, `bool`, or `int64`. A decimal column has those keys plus `precision` and `scale`, and `type` is `decimal`. `precision` and `scale` on a non-decimal column are `unknown_field`. `float` and `json_text` are `type`. Each column is built with `DelimitedColumn::try_new`, then the profile with `DelimitedProfile::try_new`. Constructor facts stay outcome `config`.

JSONL profile keys: `id`, `columns`. A dialect, BOM, header, or null key is `unknown_field`.

Each JSONL column has keys `name`, `type`, `nullable`. `type` is `utf8`, `bool`, `int64`, or `json_text`. `decimal` is `type`. Columns are built with `JsonlColumn::try_new`, then `JsonlProfile::try_new`.

### Budget

CSV and TSV keys: `batch_rows`, `batch_bytes`, `max_record_bytes`, `max_field_bytes`, `max_total_bytes`, `max_rows`, `max_columns`, `max_fields`, `deadline_ms`.

JSONL uses the same keys except `max_field_bytes`. That key on a JSONL budget is `unknown_field`.

The numbers are passed to `PullBudget::delimited` or `PullBudget::jsonl`. Those constructors call the public budget constructors and collapse a cap refusal to `budget`. `queue_batches` is not a request field.

A JSONL profile is paired only with `PullBudget::jsonl`. A delimited profile is paired only with `PullBudget::delimited`. The parent fact for a crossed pair is `profile_budget`. The command does not build that cross itself.

There is no database, password, CA, or SQL field.

## Pull

`ParentConfig::start` spawns the admitted executable. The command then calls `next_batch` until `Ok(None)` or an error.

`Ok(None)` is not the summary. The command calls `finish` only after `Ok(None)`. `finish` before that point is not a path this command takes.

On each `VerifiedBatch` the command checks, then drops the record batch:

- `batch_index` is `0, 1, 2, ...` with no gap.
- `record.num_rows()` equals the lineage row count. Cell values are not read.
- `schema_identity`, `profile_digest`, and `config_identity` are the same on every batch.
- A batch with rows has non-zero `raw_bytes` and non-zero `arrow_bytes`.
- Field `presence` is `value`, `null`, `empty`, or `missing`.
- The number of batches read is at most `max_rows`. One more batch is `protocol` and is not a summary.

The counters kept are row count, field count, presence counts, the first and last data-row line, the first and last data-row byte offset, and the sums of `raw_bytes` and `arrow_bytes`. With no data rows every one of those numbers is 0. JSONL leaves `empty` at 0. Delimited pulls leave `missing` at 0. A different presence is `protocol`.

An error from `next_batch` or `finish` discards the counters. There is no partial summary.

`finish` must agree with the fold and with the profile the command built:

| Check | Fact when it fails |
|---|---|
| `rows` equals the fold and `batches` equals the number of batches pulled | `receipt` |
| `columns` equals the profile width | `receipt` |
| field count equals `rows * columns` | `receipt` |
| `profile_id` equals the profile id | `receipt` |
| `profile_digest` equals `profile_digest()` | `receipt` |
| `config_identity` equals `config_identity(input sha256)` | `receipt` |
| `object_sha256` equals the request input sha256 | `receipt` |
| batch identities equal the receipt | `receipt` |
| `reader_protocol` is `oo.source.jsonl/v1` or `oo.source.delimited/v1` for the format | `protocol` |
| `arrow_ipc()` equals `batches > 0` | `receipt` |
| `admits_import`, `published`, and `pg_run_complete` are false | `admission` |
| `KERNEL_RESTRICTION` is `Unsupported` | `kernel` |

## Success document

Stdout is one JSON object, keys sorted, with a trailing newline. Stderr is empty.

| Field | Source |
|---|---|
| `protocol` | `oo.connectors.parse-file-result/v1` |
| `parent_protocol` | `FILE_WORKER_PARENT_PROTOCOL` |
| `reader_protocol` | receipt |
| `profile_id` | receipt |
| `profile_digest`, `config_identity`, `object_sha256`, `schema_identity` | receipt, 64 lowercase hex |
| `rows`, `columns`, `batches` | receipt |
| `arrow_ipc` | receipt |
| `admits_import`, `published`, `pg_run_complete` | receipt methods |
| `kernel_restriction` | `unsupported` |
| `raw_bytes`, `arrow_bytes` | sums of the verified batch counters |
| `lineage` | fold below |
| `lineage_sha256` | SHA-256 of the lineage preimage, 64 lowercase hex |
| `types` | profile column name, logical type, nullability; decimal adds `precision` and `scale` |

`lineage` keys: `rows`, `fields`, `value`, `null`, `empty`, `missing`, `first_line`, `last_line`, `first_byte_offset`, `last_byte_offset`.

`lineage_sha256` is SHA-256 of this UTF-8 text, with the integers in decimal and a trailing newline on every line:

```text
oo.connectors.parse-file-lineage/v1
rows=<u64>
fields=<u64>
value=<u64>
null=<u64>
empty=<u64>
missing=<u64>
first_line=<u64>
last_line=<u64>
first_byte_offset=<u64>
last_byte_offset=<u64>
raw_bytes=<u64>
arrow_bytes=<u64>
```

The preimage has no cell text, no path, and no field name. The document does not contain cell text, per-row field offsets, the input path, the executable path, or a credential.

## Errors

Stderr is one JSON object, keys sorted, with a trailing newline. Stdout is empty. The keys are exactly `exit_code`, `fact`, `group_signal`, `outcome`, `protocol`, `reaped`, `term_signal`, and `worker_token`.

`protocol` is `oo.connectors.parse-file-error/v1`. `fact` and `worker_token` are static tokens or JSON null. They are not a path, a cell, an operating-system message, or a request value. `worker_token` is the parent's static stderr token when it recorded one, and JSON null otherwise. `exit_code` and `term_signal` are JSON null when the parent did not report one. `reaped` is false when no child was waited on. `group_signal` is `not_needed`, `sent`, `skipped_identity`, or `rejected`. `sent` means `/bin/kill -KILL -- -<group>` returned success for the owned group. It is not a certificate that every descendant died.

| Exit | Outcome | When |
|---|---|---|
| 0 |  | success document on stdout |
| 2 | `usage` | arguments |
| 3 | `config` | request shape, version, format, profile, budget, `worker_path`, `path_form`, `profile_budget`, `input_too_large`, `executable_bytes`, `worker_deadline` |
| 4 | `read` | stdin, stdout, `executable_io`, `input_io`, `not_regular`, `input_changed` |
| 5 | `parent` | `spawn`, `malformed`, `trailer`, `protocol`, `ended`, `worker`, `io`, `receipt`, `admission`, `kernel` |
| 6 | `hash` | `executable_sha256` mismatch, or parent `hash_mismatch` |
| 7 | `deadline` | executable-hash clock, parent `deadline`, or parent `timed_out` |

A parent `Config` token outside the list above is fact `parent_config`, outcome `config`, exit 3. The unseen token is not copied.

`hash_mismatch` keeps the worker's exit when the parent reports it. `header_required` and a malformed delimited quote are missing completion frames: the parent fact is `ended`, and `worker_token` is the worker's static token when the parent captured it (`header_required` or `protocol`). Neither path writes a success document.

`timed_out` is the parent deadline. The parent kills and reaps. The error document carries that teardown. The next process start is a new `ParentConfig::start`. This command does not reuse a child.

## Out of scope

Import, publication, PostgreSQL, HTTP, a public authentication decision, dialect sniffing, GB18030, Parquet, Excel, and a kernel containment proof stay out of this command. `KernelRestriction::Unsupported` stays the parent state. A review of this command does not close the worker review or the parent review.
