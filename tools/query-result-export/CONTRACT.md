# query-result-export command

`query-result-export` reads one local `query-pg` stdout and writes one spreadsheet-safe CSV plus a sidecar. The producer is `query-pg` as specified by `contracts/gateway/query-pg-cli-v1.md` at the fixed baseline, plus the decimal cell frozen by `execution/decisions/numeric-query-contract-a327.md`. Stdout is JSON lines. A row line has `record` `row`, `turn`, and `cells`. A cell `kind` is `null`, `bool`, `int`, `text`, or `decimal`. `null` has no `value`. `bool` is a JSON boolean. `int` and `text` are JSON strings. `int` is the executor's canonical decimal text for int2, int4, or int8, including `-9223372036854775808`. `decimal` is the coefficient and scale object in the exact-decimal section. One summary line follows the rows. This command does not open a database, does not read SQL, and does not start `query-pg`.

`int` is not renamed and is not the exact-decimal cell. Exact decimal is kind `decimal`. Kind `numeric` is refused. This command does not decode PostgreSQL OID 1700 and does not change the producer.

`docs/10-security.md` section 6 and `REQ-SEC-02` require a prefix on CSV or Excel cells that start with `= + - @`, including when whitespace or a control character comes first. The prefix must not be treated as a change to an internal numeric value. This command implements that rule for CSV only. The baseline has no xlsx writer, and this crate adds no registry dependency, so xlsx output is unsupported.

The command does not authenticate a caller, does not issue a URL, and does not claim HTTP authorization. Citation metadata is not in `query-pg` stdout: there is no `query_id` and no fingerprint. The sidecar records that absence. It does not derive an id from `original_sha256`.

## Usage

```text
query-result-export --request <path>
query-result-export --help
```

`--help` alone prints the operator summary and exits 0. Any other argument shape exits 2. Stderr is `query-result-export: ` plus one static token. The token is never a cell, a path, SQL, or a credential. Success writes no stderr and no stdout. The artifacts are files.

## Request

The request is a regular non-symlink file of at most 262144 bytes. The bytes are UTF-8 JSON. A leading BOM, duplicate object keys, trailing commas, trailing bytes, a raw control inside a string, nesting deeper than 8, and more than 8192 JSON values are refused. Numbers are unsigned integers with no sign, fraction, or exponent, except a leading `-` on a non-zero integer. That negative token is legal only as `scale` on a decimal cell. `-0`, a fraction, and an exponent are syntax errors. Unknown fields are refused.

`version` is `oo.query-result-export/v1`.

`source_file` is the `query-pg` stdout. `output_dir` is an existing directory. Both paths are checked one component at a time. A symlink component is refused. The final source component must be a regular file. The final output component must be a directory. The command does not canonicalize through a symlink.

`columns` has 1 to 512 strings. Each name is 1 to 256 UTF-8 bytes and contains no NUL. Names are unique. `query-pg` stdout has no column names. These strings are the caller-requested header. They are text, so the formula rule applies to them. They are not typed `int` cells.

There is no field for a URL, a token, an xlsx path, or a caller-supplied `query_id`.

## Source

The source is at most 33554432 bytes. It contains no raw CR. Lines are split on LF. One trailing empty line is allowed. Any other empty line is refused. Each line is one JSON object with the same grammar limits as the request.

The document is one turn: zero or more row lines, then one summary line. Every line shares that summary's `turn`. `turn` fits `u32`. A second summary, a row after the summary, or a missing summary is refused.

Every data row has the same width. That width equals `columns`. Zero rows are allowed when the summary's `returned_rows` is 0 and `result_bytes` is 0. The header is still written. A width that does not match `columns` is a request failure. Rows that disagree with each other are a source failure.

A `null` cell has only `kind`. A `bool` cell has `value` true or false. An `int` cell's `value` is canonical int2/int4/int8 text: the `Display` form of an `i64`, so an optional leading `-`, then digits, with no leading zero except for `0`, and no `-0`, no `+`, and no fraction. A `decimal` cell is the object in the next section. Any other `kind`, including `numeric`, is refused. A decimal object that uses `value` instead of `negative`, `coefficient`, and `scale` is refused. A non-null column keeps one kind. Nulls may mix with that kind. Two different non-null kinds in one column are refused.

## Exact decimal

A `decimal` cell has exactly `kind`, `negative`, `coefficient`, and `scale`. `negative` is a bool. `coefficient` is a string of ASCII digits, length 1 through 8192, with no leading zero except the single character `0`. `scale` is an integer in `-8192..=8192`. The triple must already be canonical. This command does not strip zeros and does not clear a sign. `negative: true` with coefficient `0` is a source failure. A longer coefficient or a scale outside that range is a source failure.

Plain text follows `ExactDecimal`'s display. Scale `0` is the coefficient. A positive scale inserts the point that many digits from the right, or `0.` and zeros when the coefficient is shorter. A negative scale writes the coefficient and then that many zeros. A leading `-` is written only when `negative` is true. The CSV field is that plain text and nothing else.

`result_bytes` must equal the producer cell-byte sum: null is 0, true is 4, false is 5, `int` or `text` is the UTF-8 length of the cell text, and `decimal` is the UTF-8 length of the canonical plain text. `returned_rows` must equal the number of row lines. A mismatch is a source failure and writes nothing.

At most 2000 rows and 512 cells in a row are accepted. A larger source, CSV, or sidecar is a budget failure and writes nothing.

Summary fields are the producer set: `status` (`succeeded`, `partial`, `failed`), `truncated`, `returned_rows`, `result_bytes`, `rollback_confirmed`, `socket_dropped`, `end_audit_ok`, `original_sha256`, `executed_sha256`, `stop`, `sqlstate`. Hashes are 64 lowercase hex digits. `stop` is null or `timeout`, `canceled`, `type_refused`, `database`, `protocol`. `sqlstate` is null or five ASCII uppercase letters or digits.

## Formula transformation

The transformation id is `apostrophe-prefix-after-leading-whitespace-or-control`.

It applies to caller column names and to `text` cells. It does not apply to `int`, `decimal`, `bool`, `null`, or empty CSV fields that came from null.

A text value is formula-prone when, after removing a leading run of characters for which `char::is_whitespace()` or `char::is_control()` is true, or which are U+FEFF, the next character is `=` (U+003D), `+` (U+002B), `-` (U+002D), or `@` (U+0040). `is_whitespace` is Unicode White_Space. `is_control` is Unicode category Cc, which covers C0, DEL, and C1. The leading run skips U+FEFF and no other format character.

The transformed text is U+0027 APOSTROPHE followed by the original text. No other character is inserted, deleted, or rewritten. The apostrophe remains in the CSV field. This command does not claim that a spreadsheet hides it.

A typed `int` stays the canonical digit text, including a leading minus. `-9223372036854775808` is written as those 20 bytes. It is not an expression. A typed `decimal` stays the canonical plain text, including a leading minus, and receives no apostrophe. A `text` cell whose characters are `-2+3`, `-1`, or a minus after whitespace, a control, or U+FEFF is formula-prone and receives the apostrophe. One gold text cell is U+FEFF followed by `=1+1`. Its CSV field is U+0027, then U+FEFF, then `=1+1`. The file does not begin with a BOM. `true` and `false` are written as those letters.

## CSV

The CSV is UTF-8 with no BOM. Records end with CR LF. Fields are separated by a comma. A field is wrapped in double quotes when it contains `"` (U+0022), a comma, CR, or LF. A quote inside a quoted field is doubled. Other bytes, including tab, Unicode, and a leading apostrophe, are copied as UTF-8. Quoting is not used for any other reason. A null cell and an empty text cell are both an empty field. The CSV does not prove which one was null.

The file name is `export.csv`.

## Sidecar and source copy

Two more files are created in `output_dir`, only if all three names are absent:

| File | Bytes |
| --- | --- |
| `export.source.jsonl` | the exact source bytes |
| `export.csv` | the CSV |
| `export.sidecar.json` | one JSON object and a trailing LF |

Creation uses `create_new`. An existing name, including a symlink, is refused before any of the three is created. A failure after a create removes only the names this call created. Unrelated files stay. Each successful file is `sync_all`'d. That is not a power-failure claim and not a distributed lock.

The sidecar is compact UTF-8 JSON, keys in the order below, no extra whitespace. Strings escape `"`, `\`, and control characters as short JSON escapes or `\u00xx`. Other Unicode is raw UTF-8.

`complete` is true only when `status` is `succeeded`, `truncated` is false, `stop` and `sqlstate` are null, `end_audit_ok` is true, `rollback_confirmed` is true, `socket_dropped` is false, and the row and byte counts match. `partial` is the opposite boolean. A truncated or otherwise unclean but internally consistent summary still writes the three files, with `complete` false. That export is not a complete result. An inconsistent summary writes nothing.

`source_sha256` is SHA-256 of the exact source bytes. `csv_sha256` is SHA-256 of the CSV bytes. `original_sha256` and `executed_sha256` are copied from the summary. `citation.in_source` is false and `citation.query_id` is null. `http_auth` and `authenticated` are false. `xlsx` is `unsupported`. `format` is `csv`.

`columns[]` records the caller name, the observed kind (`int`, `decimal`, `bool`, `text`, `null` when every cell was null, or `none` when there were no rows), `nulls`, and `empty_text`. `null_cells`, `empty_text_cells`, and `formula_cells` are `[row, column]` pairs in row-major order, indexes from 0. `int_cells[]` repeats each typed integer as `row`, `column`, and the exact `text`. `decimal_cells[]` follows `int_cells` and repeats each typed decimal as `row`, `column`, `negative`, `coefficient`, `scale`, and `text`. `text` there is the canonical plain text copied into the CSV. Those arrays are how null is distinguished from empty text, how an integer is shown to be unescaped, and how a decimal keeps its coefficient and scale. The source copy remains the lossless typed payload.

`formula_transformation` is the id above. `formula_prefixed` is the number of text cells that received an apostrophe. Column-name escapes are visible in the CSV header and are not counted as cells.

## Exits

| Code | Token | When |
| --- | --- | --- |
| 0 | | The three files were written and `complete` is true. Stderr is empty. |
| 1 | `partial` | The three files were written and `complete` is false. |
| 1 | `io` | A file create succeeded and a later write or sync failed. Created names from this call are removed. |
| 2 | `usage` | The arguments are not the usage above. |
| 3 | `request` | The request document, column list, or row width does not match this contract. |
| 3 | `budget` | A size, depth, node, row, or column budget is exceeded. No artifact is left. |
| 4 | `path` | A path is missing, not the required file type, a symlink component, or an output name that already exists. |
| 5 | `source` | The bytes are not one coherent `query-pg` stdout of the kinds above. |

## Fixture

`tests/fixtures/query-clerk-order_doc-star.stdout` is the compiled `query-pg` stdout copied from `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a226/SP11/reviewed-pg-composition/live/steps/query-clerk-order_doc-star.stdout`. Its SHA-256 is `54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb` and its length is 1027 bytes. The cells are `shop.order_doc` in table order: `order_id`, `customer_id`, `memo`, `posted`, `amount`, `clerk_note`, including int8 minimum and maximum, null, and the UTF-8 labels from that fixture. `tests/fixtures/hand-formula.jsonl` is hand-authored in that same grammar. It is not a second compiled receipt. `tests/fixtures/golden-clerk.csv` is the hand-written RFC 4180 bytes for the compiled stdout. Decimal examples in the unit tests use the frozen cell above. They are not compiled `query-pg` receipts.

This command does not close SP05-W06, AT-FILE-05, or AT-SEC-03. Download authorization, retention, deletion, reaper, and xlsx remain parent work.
