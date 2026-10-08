# local-analysis command

`local-analysis` reads one verified `query-result-export` directory and one local audit lineage file. It checks hashes, the derived `query_id`, and completion, then writes a bounded exact analysis. This command does not open a database, does not read SQL, does not start `query-pg` or `query-result-export`, and does not hold a database credential.

The completion definition of this card is Plan SP09-W05 and AT-API-04 in full: read-only authorized Parquet, local DuckDB and Arrow analysis, Chinese charts, formula-safe CSV, Excel, source manifest, resource-budget isolation of native code, and a killable native exit. Lineage has to be checkable. An output with no verifiable input is not citable and is not business evidence.

This command does not close that definition. Root `Cargo.toml`, root `Cargo.lock`, and `supply-chain/dependency-baseline.json` do not provide DuckDB, Arrow, Parquet, an Excel writer, or a CJK font. `supply-chain/dependency-baseline.md` forbids the rust `duckdb` crate on `oo-analysis`. A DuckDB CLI is not a crate this tree ships. `DEPENDENCY-PROPOSAL.md` is the admission proposal. `audited` stays false. CSV, SVG, and the manifest below are the part that can run with no new crate. They are not a substitute for DuckDB, Arrow, Parquet, or Excel, and they do not shrink the definition above.

## Usage

```text
local-analysis --request <path>
local-analysis --help
```

`--help` alone prints the operator summary and exits 0. Any other argument shape exits 2. Stderr is `local-analysis: ` plus one static token. The token is never a cell, a path, SQL, a credential, or a `query_id`. Success writes no stderr and no stdout. The artifacts are files. Exit 1 with `uncited` or `canceled` or `io` also writes no stdout.

## Request

The request is a regular non-symlink file of at most 262144 bytes. The bytes are UTF-8 JSON. A leading BOM, duplicate object keys, trailing commas, trailing bytes, a raw control inside a string, nesting deeper than 8, and more than 400000 JSON values are refused. Numbers are unsigned integers with no sign, fraction, or exponent, except a leading `-` on a non-zero integer. `-0`, a fraction, and an exponent are syntax errors. Unknown fields are refused.

`version` is `oo.local-analysis/v1`.

`export_dir` is an existing directory that already contains `export.source.jsonl`, `export.csv`, and `export.sidecar.json` from `query-result-export`. `lineage_file` is the lineage document below. `output_dir` is an existing directory. Paths are checked one component at a time. A symlink component is refused. The command does not canonicalize through a symlink.

`operation` is `sum` or `count`. `group_column` and `value_column` are indexes into the export width, from 0. `group_header` and `value_header` are 1 to 256 UTF-8 bytes, with no NUL. `chart_title` is 1 to 256 UTF-8 bytes, with no NUL. Those three strings are text. The formula rule applies to the two headers when they are written to CSV. The title is written to the SVG as XML text, not as a formula and not as a URL.

`cancel_file` is null or a path. When the path names a regular file at the start of the command or again immediately before the output files are created, the command writes nothing and exits `canceled`. The file's bytes are not read. A symlink is refused.

`engine` is `rust_exact` or `duckdb`. `duckdb` exits `engine` and writes nothing. Any other string is a request failure. `xlsx` true exits `xlsx` and writes nothing. `parquet` true exits `parquet` and writes nothing. Those three refusals are the admitted behavior while the proposal is not audited. They are not an implementation of DuckDB, Excel, or Parquet.

`engine` is checked after `parquet` and `xlsx`. A request that sets more than one of those asks is refused on the first of `parquet`, then `xlsx`, then `engine`.

## Lineage

`version` is `oo.local-analysis-lineage/v1`. `trust` is `local_operator_declared`. Any other trust string is `lineage`. This command does not authenticate a caller, does not check a signature, and does not claim HTTP authorization. `authorization` in the manifest is `not_claimed`. `authenticated` is false.

`query_id` is the caller's copy of the id derived from the receipt. `receipt_sha256` is 64 lowercase hex digits and is the SHA-256 of the exact UTF-8 bytes of `receipt_body` after JSON string decoding. A mismatch is `hash` and writes nothing.

`receipt_body` is one `oo.gateway.query-pg-audit/v1` pair: an `admit` line and a `complete` line, as `tools/answer-citations/CONTRACT.md` describes that pair. One trailing empty line is allowed. A raw CR is refused. The derived id is `q_` plus `fingerprint_hex` when that fingerprint is 6 to 40 lowercase hex digits. The same rule as answer-citations is reused. This command does not invent a second id. The caller's `query_id` must equal that derived id.

The pair's `original_sha256` and `executed_sha256` must equal the export sidecar. The pair's `status`, `truncated`, `returned_rows`, `result_bytes`, `rollback_confirmed`, `socket_dropped`, `stop`, and `sqlstate` must equal the sidecar. Role, project, policy id, fingerprint, backend pid, and backend start must match inside the pair. Policy id and backend identity are not copied into the manifest.

## Export check

Each of the three export files is at most 1048576 bytes. A larger file is `budget` and writes nothing. `export.source.jsonl` is hashed and must equal sidecar `source_sha256`. `export.csv` must equal sidecar `csv_sha256`. Declared `source_bytes` and `csv_bytes` must equal the file lengths. Sidecar `version` is `oo.query-result-export/v1`, `format` is `csv`, `xlsx` is `unsupported`, `http_auth` is false, `authenticated` is false, and `citation` is `{"in_source":false,"query_id":null}`. The source is reparsed. Row width must equal the sidecar `columns` array length. `returned_rows`, `parsed_rows`, and `result_bytes` must match that parse. The cell grammar, decimal canonical form, and `result_bytes` weights are the exporter contract: null 0, true 4, false 5, otherwise the UTF-8 length of the int, text, or canonical decimal text.

The sidecar `complete` flag must equal the recomputed clean success: `status` `succeeded`, `truncated` false, `stop` and `sqlstate` null, `end_audit_ok` true, `rollback_confirmed` true, `socket_dropped` false, and the row and byte counts match. `partial` must be the opposite boolean. A sidecar that disagrees with the bytes is `source` and writes nothing. A receipt that disagrees with the sidecar is `lineage` and writes nothing.

A clean success that also binds the receipt is citable. An internally consistent export that is not a clean success, and whose receipt agrees, is written with `citable` false. That output is not business evidence.

## Analysis

Values come from the typed source, not from the CSV text. `sum` requires one non-null kind in `value_column`, either `int` or `decimal`. Mixed kinds are `source`. Null values are excluded and counted. `int` is base-10, including `-9223372036854775808`, and is a scale of 0. Decimals keep coefficient and scale. The group sum aligns to the maximum scale by appending zeros, then adds the signed coefficients as decimal strings. No binary float is used. The result scale is that maximum. A zero sum is non-negative, coefficient `0`, and the plain text keeps the scale (`0.0000` at scale 4). More than 16384 coefficient digits is `budget`.

`count` counts rows with a non-null group key. It does not read the value kind. The count is a scale-0 integer.

A null group key is excluded and counted. Group keys keep first-seen order. At most 64 groups. A text group key is the raw text. An int or decimal group key is its canonical plain text and is not formula-prefixed. At most 2000 source rows and 512 columns are accepted.

## Formula transformation

The transformation id is `apostrophe-prefix-after-leading-whitespace-or-control`, the exporter's id. It applies to `group_header`, `value_header`, and text group keys. It does not apply to int, decimal, bool, count, or sum fields.

A text value is formula-prone when, after removing a leading run of characters for which `char::is_whitespace()` or `char::is_control()` is true, or which are U+FEFF, the next character is `=` (U+003D), `+` (U+002B), `-` (U+002D), or `@` (U+0040). The transformed text is U+0027 APOSTROPHE followed by the original text. No other character is inserted, deleted, or rewritten. The apostrophe remains in the CSV field. Typed negatives do not receive it.

## CSV

`analysis.csv` is UTF-8 with no BOM. Records end with CR LF. Quoting follows the exporter rule: a field is wrapped in double quotes when it contains a quote, a comma, CR, or LF, and a quote inside is doubled. Two columns: the transformed group header and the transformed value header, then one row per group. The value field is the canonical plain text. At most 65536 bytes.

## SVG

`analysis.svg` is UTF-8 XML. The only URI is the SVG namespace `http://www.w3.org/2000/svg` on the root. It is not fetched. There is no DOCTYPE, script, style element, image, foreignObject, or href. Text is XML-escaped. Characters illegal in XML 1.0 are omitted from text nodes and the manifest warning `svg_label_omitted` is set. The title is the request title, copied as characters, so CJK scalars are present as UTF-8. The `query_id` is drawn as text. Bar length is the integer ratio of the absolute aligned coefficient to the maximum, times 400, with a minimum of 1 when the value is non-zero and the ratio truncates to 0. A zero value has length 0. Negative values keep the minus sign in the label. The description says `数值已核对` when citable and `不可引用` when not. It always says `DuckDB、Arrow、Excel未准入` and `字体未随包提供`. An empty group list draws `无分组`. At most 65536 bytes.

No CJK font file is bundled. `cjk_font` is `not_bundled`. A font-family name is a label, not a shipped font.

## Manifest

`analysis.manifest.json` is one compact UTF-8 object and a trailing LF, at most 65536 bytes. Keys are in this order:

`version` `oo.local-analysis/v1`, `closes_sp09_w05` false, `citable`, `authorization` `not_claimed`, `authenticated` false, `trust` `local_operator_declared`, `lineage` `bound`, `query_id`, `role`, `project`, `engine` `rust_exact`, `duckdb` `not_admitted`, `arrow` `not_admitted`, `parquet` `not_admitted`, `xlsx` `not_admitted`, `svg` `rust_native`, `cjk_font` `not_bundled`, `formula_transformation`, `source_sha256`, `csv_sha256`, `sidecar_sha256`, `receipt_sha256`, `original_sha256`, `executed_sha256`, `analysis_csv_sha256`, `svg_sha256`, `export_complete`, `truncated`, `operation`, `group_column`, `value_column`, `group_count`, `null_groups`, `null_values`, `warnings`, `groups`.

`groups[]` has `key`, `text`, `negative`, `coefficient`, and `scale`. `text` is the canonical plain value. `warnings` is an array of zero or more of `duckdb_not_admitted`, `arrow_not_admitted`, `xlsx_not_admitted`, `cjk_font_not_bundled`, `null_group`, `null_value`, `uncited`, `svg_label_omitted`. The four admission warnings are always present. `uncited` is present only when `citable` is false.

`closes_sp09_w05` is always false. This file does not accept SP09-W05, AT-API-04, the parent phase, or the goal.

## Atomic write

The three final names and the three `.partial` names must be absent. The command writes the partials, `sync_all`s them, checks the cancel file again, then renames. A failure after a create removes only names this call created. A pre-existing file, including a directory that blocked a later create, is left in place. That is not a power-failure claim. A killed process can leave a partial name. The next call refuses that name and does not delete it.

## Exits

| Code | Token | When |
| --- | --- | --- |
| 0 | | The three files were written and `citable` is true. Stderr is empty. |
| 1 | `uncited` | The three files were written and `citable` is false. |
| 1 | `canceled` | The cancel file was a regular file. No name created by this call remains. |
| 1 | `io` | A create succeeded and a later write, sync, or rename failed. Created names are removed. |
| 2 | `usage` | The arguments are not the usage above. |
| 3 | `request` | The request document is refused. |
| 3 | `budget` | A size, depth, node, row, column, group, or digit budget is exceeded. No final artifact is left. |
| 3 | `engine` | `engine` is `duckdb`. Nothing is written. |
| 3 | `xlsx` | `xlsx` is true. Nothing is written. |
| 3 | `parquet` | `parquet` is true. Nothing is written. |
| 4 | `path` | A path is missing, not the required file type, a symlink, or an output name exists. |
| 4 | `hash` | An export or receipt SHA-256 does not match the declared hash or the bytes. |
| 4 | `lineage` | The receipt, `query_id`, or outcome binding does not match. |
| 5 | `source` | The export bytes are not one coherent exporter result. |

## What this command does not do

It does not read Parquet. It does not start DuckDB. It does not write xlsx. It does not bundle a CJK font. It does not kill a native child, because it does not spawn one. It does not claim the export's `query_id` was in the source file. It does not close SP09-W05.
