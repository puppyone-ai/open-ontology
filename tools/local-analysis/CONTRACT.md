# local-analysis command

`local-analysis` reads one verified `query-result-export` directory and one local audit lineage file. It checks hashes, the derived `query_id`, and completion, then writes a bounded analysis. This command does not open a database, does not read SQL, does not start `query-pg` or `query-result-export`, and does not hold a database credential.

Plan SP09-W05 and AT-API-04 are the card definition: read-only authorized Parquet, local DuckDB and Arrow analysis, Chinese charts, formula-safe CSV and Excel, a source manifest, resource limits on the native CLI, and a kill of that CLI when cancel is requested. An output with no verifiable input is not citable and is not business evidence.

`closes_sp09_w05` is false. `audited` is false. This command does not accept SP09-W05, AT-API-04, the parent phase, or the goal. It does not edit the root manifests or `supply-chain/dependency-baseline.json`. The child crate pins Arrow and Parquet 60.0.0, `rust_xlsxwriter` 0.99.1, the DuckDB CLI 1.5.6 linux-arm64 archive, and Noto Sans CJK SC Sans2.004. Those pins are not an owner audit. `reviewers_assigned` stays empty.

The rust `duckdb` crate is not a dependency. DuckDB runs only as the pinned CLI. Its unpacked SHA-256 is `native/duckdb-cli.json` field `unpacked_sha256`. The font SHA-256 is `native/font.json` field `otf_sha256`. A value that is not 64 lowercase hex digits, or a file whose SHA-256 differs, is `engine` and writes nothing. The tool does not download either file.

## Usage

```text
local-analysis --request <path>
local-analysis --help
```

`--help` alone prints the operator summary and exits 0. Any other argument shape exits 2. Stderr is `local-analysis: ` plus one static token. The token is never a cell, a path, SQL, a credential, or a `query_id`. Success writes no stderr and no stdout. Exit 1 with `uncited` or `canceled` or `io` also writes no stdout.

## Request

The request is a regular non-symlink file of at most 262144 bytes. The bytes are UTF-8 JSON. A leading BOM, duplicate object keys, trailing commas, trailing bytes, a raw control inside a string, nesting deeper than 8, and more than 400000 JSON values are refused. Numbers are unsigned integers with no sign, fraction, or exponent, except a leading `-` on a non-zero integer. `-0`, a fraction, and an exponent are syntax errors. Unknown fields are refused. `duckdb_cli` and `font_file` may be absent; absence means null.

`version` is `oo.local-analysis/v1`.

`export_dir` is an existing directory that already contains `export.source.jsonl`, `export.csv`, and `export.sidecar.json` from `query-result-export`. `lineage_file` is the lineage document below. `output_dir` is an existing directory. Paths are checked one component at a time. A symlink component is refused. The command does not canonicalize through a symlink.

`operation` is `sum` or `count`. `group_column` and `value_column` are indexes into the export width, from 0. `group_header` and `value_header` are 1 to 256 UTF-8 bytes, with no NUL. `chart_title` is 1 to 256 UTF-8 bytes, with no NUL. Those three strings are text. The formula rule applies to the two headers when they are written to CSV or Excel. The title is written to the SVG as XML text.

`cancel_file` is null or a path. When the path names a regular file at the start of the command, again before analysis, again while the DuckDB child is running, or again immediately before the output files are renamed, the command writes nothing durable and exits `canceled`. The file's bytes are not read. A symlink is refused. A missing cancel file is not a path error.

`engine` is `rust_exact` or `duckdb`. Any other string is a request failure. `rust_exact` sums in digit strings and does not spawn DuckDB. `duckdb` requires `duckdb_cli` and `font_file`. It writes Parquet and Arrow IPC for the included source rows, spawns the pinned CLI, and publishes only when the CLI's grouped sum text matches the digit-string sum. A mismatch, a missing CLI, a digest mismatch, a non-zero CLI exit, or a decimal that does not fit `decimal128(38, scale)` is `engine` and writes nothing. There is no fallback that labels a Rust sum as DuckDB.

`xlsx` true writes `analysis.xlsx`. `parquet` true writes `analysis.parquet` and `analysis.arrow` even when the engine is `rust_exact`. `engine` `duckdb` always writes those two files. A false flag does not delete an older file; this call simply does not create that name. Writer failures use token `parquet` or `xlsx` and leave no name this call created.

Check order before the export is opened: null `duckdb_cli` when the engine is `duckdb`; CLI digest; null `font_file` when the engine is `duckdb`; font digest when `font_file` is a string. A missing font path is `path`. A font digest mismatch is `engine`.

## Lineage

`version` is `oo.local-analysis-lineage/v1`. `trust` is `local_operator_declared`. Any other trust string is `lineage`. This command does not authenticate a caller, does not check a signature, and does not claim HTTP authorization. `authorization` in the manifest is `not_claimed`. `authenticated` is false.

`query_id` is the caller's copy of the id derived from the receipt. `receipt_sha256` is 64 lowercase hex digits and is the SHA-256 of the exact UTF-8 bytes of `receipt_body` after JSON string decoding. A mismatch is `hash` and writes nothing.

`receipt_body` is one `oo.gateway.query-pg-audit/v1` pair: an `admit` line and a `complete` line, as `tools/answer-citations/CONTRACT.md` describes that pair. One trailing empty line is allowed. A raw CR is refused. The derived id is `q_` plus `fingerprint_hex` when that fingerprint is 6 to 40 lowercase hex digits. The caller's `query_id` must equal that derived id.

The pair's `original_sha256` and `executed_sha256` must equal the export sidecar. The pair's `status`, `truncated`, `returned_rows`, `result_bytes`, `rollback_confirmed`, `socket_dropped`, `stop`, and `sqlstate` must equal the sidecar. Role, project, policy id, fingerprint, backend pid, and backend start must match inside the pair. Policy id and backend identity are not copied into the manifest.

## Export check

Each of the three export files is at most 1048576 bytes. A larger file is `budget` and writes nothing. `export.source.jsonl` is hashed and must equal sidecar `source_sha256`. `export.csv` must equal sidecar `csv_sha256`. Declared `source_bytes` and `csv_bytes` must equal the file lengths. Sidecar `version` is `oo.query-result-export/v1`, `format` is `csv`, `xlsx` is `unsupported`, `http_auth` is false, `authenticated` is false, and `citation` is `{"in_source":false,"query_id":null}`. The exporter's `xlsx` field stays `unsupported` because that producer does not write a workbook. This command's own workbook is a separate file.

The source is reparsed. Row width must equal the sidecar `columns` array length. `returned_rows`, `parsed_rows`, and `result_bytes` must match that parse. The cell grammar, decimal canonical form, and `result_bytes` weights are the exporter contract: null 0, true 4, false 5, otherwise the UTF-8 length of the int, text, or canonical decimal text.

The sidecar `complete` flag must equal the recomputed clean success: `status` `succeeded`, `truncated` false, `stop` and `sqlstate` null, `end_audit_ok` true, `rollback_confirmed` true, `socket_dropped` false, and the row and byte counts match. `partial` must be the opposite boolean. A sidecar that disagrees with the bytes is `source` and writes nothing. A receipt that disagrees with the sidecar is `lineage` and writes nothing.

A clean success that also binds the receipt is citable. An internally consistent export that is not a clean success, and whose receipt agrees, is written with `citable` false. That output is not business evidence.

## Analysis

Values come from the typed source, not from the CSV text. `sum` requires one non-null kind in `value_column`, either `int` or `decimal`. Mixed kinds are `source`. Null values are excluded and counted. `int` is base-10, including `-9223372036854775808`, and is a scale of 0. Decimals keep coefficient and scale. The group sum aligns to the maximum scale by appending zeros, then adds the signed coefficients as decimal strings. No binary float is used. The result scale is that maximum. A zero sum is non-negative, coefficient `0`, and the plain text keeps the scale (`0.0000` at scale 4). More than 16384 coefficient digits is `budget`.

`count` counts rows with a non-null group key. It does not read the value kind. The count is a scale-0 integer.

A null group key is excluded and counted. Group keys keep first-seen order. At most 64 groups. A text group key is the raw text. An int or decimal group key is its canonical plain text and is not formula-prefixed. At most 2000 source rows and 512 columns are accepted.

## Parquet and Arrow

When Parquet is requested, or the engine is `duckdb`, the command writes one Arrow record batch and the same batch as Snappy Parquet. Columns are `group_key` (utf8), `ordinal` (int32, first-seen group order), and `coefficient` (`decimal128(38, scale)`, the signed row value aligned to the file scale). Null groups and null values are omitted. `count` stores coefficient `1` at scale 0. The file scale is the group scale. A coefficient that does not fit 38 decimal digits, or a scale outside 0..=38, is `engine`. The two files are one series. They are not a cast of DuckDB's private Arrow buffers.

DuckDB reads that Parquet. The SQL is fixed by this program: one thread, `memory_limit` 256MB, extension auto-install and auto-load off, `allowed_directories` limited to the private temp directory, `enable_external_access` false, `lock_configuration` true, and no remote URI. `enable_external_access` false alone blocks `read_parquet`; the allowlist is what keeps the read inside the temp directory. Cell text is not interpolated. A temp path that contains a quote or is not UTF-8 is `engine`. The CLI environment is cleared. The child is killed if cancel becomes a regular file or if the wait exceeds 30 seconds (`budget` for the timeout). The temp directory is removed on every exit. The published numbers are the CLI's varchar sums after they match the digit-string groups, including scale. A mismatch writes nothing.

## Formula transformation

The transformation id is `apostrophe-prefix-after-leading-whitespace-or-control`, the exporter's id. It applies to `group_header`, `value_header`, and text group keys in both CSV and Excel. It does not apply to int, decimal, bool, count, or sum fields.

A text value is formula-prone when, after removing a leading run of characters for which `char::is_whitespace()` or `char::is_control()` is true, or which are U+FEFF, the next character is `=` (U+003D), `+` (U+002B), `-` (U+002D), or `@` (U+0040). The transformed text is U+0027 APOSTROPHE followed by the original text. No other character is inserted, deleted, or rewritten. Typed negatives do not receive it.

Excel cells are shared strings written with an `@` text format. The workbook never calls a formula writer. A typed minus remains the minus character inside the string.

## CSV

`analysis.csv` is UTF-8 with no BOM. Records end with CR LF. Quoting follows the exporter rule: a field is wrapped in double quotes when it contains a quote, a comma, CR, or LF, and a quote inside is doubled. Two columns: the transformed group header and the transformed value header, then one row per group. The value field is the canonical plain text. At most 65536 bytes.

## SVG

`analysis.svg` is UTF-8 XML. The only `http` URL is the SVG namespace `http://www.w3.org/2000/svg` on the root. It is not fetched. There is no DOCTYPE, script, image, foreignObject, or href. When a verified font is copied, one `style` element may contain `@font-face` whose `src` is the relative URL `NotoSansCJKsc-Regular.otf`. That is not an `http` URL and it is not fetched by this command. Text is XML-escaped. Characters illegal in XML 1.0 are omitted from text nodes and the manifest warning `svg_label_omitted` is set. The title is the request title, copied as characters. The `query_id` is drawn as text. Bar length is the integer ratio of the absolute aligned coefficient to the maximum, times 400, with a minimum of 1 when the value is non-zero and the ratio truncates to 0. A zero value has length 0. Negative values keep the minus sign in the label.

A citable chart's description starts with `数值已核对`. An uncitable chart's description starts with `不可引用` and does not contain `数值已核对`. A run that did not call DuckDB says `本运行未调用DuckDB`. A run that called DuckDB and matched says `DuckDB合计与精确十进制一致`. A missing font says `字体未随包提供`. A verified font says `字体已校验`. An empty group list draws `无分组`. At most 65536 bytes.

The copied font file name is `NotoSansCJKsc-Regular.otf`. Its bytes are the verified input, not a download.

## Manifest

`analysis.manifest.json` is one compact UTF-8 object and a trailing LF, at most 65536 bytes. Keys are in this order:

`version` `oo.local-analysis/v1`, `closes_sp09_w05` false, `audited` false, `citable`, `authorization` `not_claimed`, `authenticated` false, `trust` `local_operator_declared`, `lineage` `bound`, `query_id`, `role`, `project`, `engine` (`rust_exact` or `duckdb`), `duckdb` (`not_invoked` or `pinned_unaudited`), `arrow` (`not_written` or `pinned_unaudited`), `parquet` (`not_written` or `pinned_unaudited`), `xlsx` (`not_written` or `pinned_unaudited`), `svg` `rust_native`, `cjk_font` (`not_bundled` or `pinned_unaudited`), `formula_transformation`, `source_sha256`, `csv_sha256`, `sidecar_sha256`, `receipt_sha256`, `original_sha256`, `executed_sha256`, `analysis_csv_sha256`, `svg_sha256`, `parquet_sha256`, `arrow_sha256`, `xlsx_sha256`, `font_sha256`, `duckdb_binary_sha256`, `export_complete`, `truncated`, `operation`, `group_column`, `value_column`, `group_count`, `null_groups`, `null_values`, `warnings`, `groups`.

Absent hashes are JSON null. Present hashes are 64 lowercase hex digits of the published bytes. `duckdb_binary_sha256` is the unpacked CLI digest when the engine is `duckdb`, otherwise null.

`groups[]` has `key`, `text`, `negative`, `coefficient`, and `scale`. `warnings` always starts with `admission_not_audited`, then exactly one of `duckdb_not_invoked` or `duckdb_pinned_unaudited`, one of `arrow_not_written` or `arrow_pinned_unaudited`, one of `parquet_not_written` or `parquet_pinned_unaudited`, one of `xlsx_not_written` or `xlsx_pinned_unaudited`, and one of `cjk_font_not_bundled` or `cjk_font_pinned_unaudited`. Then zero or more of `null_group`, `null_value`, `uncited`, `svg_label_omitted`. `uncited` is present only when `citable` is false.

`pinned_unaudited` means this run used the pinned bytes. It does not mean an owner audited them.

## Atomic write

Every final name this call will create, and its `.partial` sibling, must be absent. The command writes the partials, `sync_all`s them, checks the cancel file again, then renames. A failure after a create removes only names this call created. A pre-existing file, including a directory that blocked a later create, is left in place. A killed process can leave a partial name. The next call refuses that name and does not delete it.

## Exits

| Code | Token | When |
| --- | --- | --- |
| 0 | | The files were written and `citable` is true. Stderr is empty. |
| 1 | `uncited` | The files were written and `citable` is false. |
| 1 | `canceled` | Cancel was requested. No name created by this call remains. The DuckDB child is killed when it was running. |
| 1 | `io` | A create succeeded and a later write, sync, or rename failed. Created names are removed. |
| 2 | `usage` | The arguments are not the usage above. |
| 3 | `request` | The request document is refused. |
| 3 | `budget` | A size, depth, node, row, column, group, digit, or CLI time budget is exceeded. No final artifact is left. |
| 3 | `engine` | The CLI or font pin is unusable, the decimal does not fit `decimal128`, or the CLI result does not match. Nothing is written. |
| 3 | `xlsx` | Workbook construction failed. Nothing is written. |
| 3 | `parquet` | Parquet or Arrow IPC construction failed. Nothing is written. |
| 4 | `path` | A path is missing, not the required file type, a symlink, or an output name exists. |
| 4 | `hash` | An export or receipt SHA-256 does not match. |
| 4 | `lineage` | The receipt, `query_id`, or outcome binding does not match. |
| 5 | `source` | The export bytes are not one coherent exporter result. |

## What this command does not do

It does not close SP09-W05. It does not set `audited` true. It does not install a DuckDB extension, open a remote URI, or read a database credential. It does not execute spreadsheet formulas. It does not download a font or a CLI at runtime.
