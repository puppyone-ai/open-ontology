# Query result adapter v1

Protocol id: `oo.benchmark.query-result-adapter/v1`.

This document is the frozen input and output of one adapter. The input is one explicit question header plus one `query-pg` stdout. The output is one typed actual result. The result comparator reads that object as the `actual` side of `oo.benchmark.result-comparison/v1`.

This program does not read an expected result, a gold file, or a comparison document. It does not decide match or mismatch. It does not call a model, compute pass^k, or score prose. It does not accept a benchmark suite, a 300-question bank, or a release.

The crate is a standalone workspace. It links `oo-core` from the fixed source archive and uses serde 1.0.229 and serde_json 1.0.151. It does not edit the root manifest, the root lock, or the comparator.

## Command

```text
query-result-adapter
query-result-adapter --help
```

No other arguments. `--help` and `-h` write the help text below to stdout, write nothing to stderr, do not read stdin, and exit 0. Any other argument list writes `query-result-adapter: usage` to stderr, writes nothing to stdout, does not read stdin, and exits 2.

Help text, including the final newline:

```text
query-result-adapter reads oo.benchmark.query-result-adapter/v1 JSONL from stdin.
It writes one typed actual result for the result comparator to stdout.
exit 0 actual
exit 2 config
```

With no arguments the process reads stdin until EOF, capped at 1_048_577 bytes. A longer stream is refused before parse. The bytes must be UTF-8. A successful read writes one compact JSON object and a final newline to stdout. Stderr is empty on success.

| Exit | Stdout | Stderr |
| --- | --- | --- |
| 0 | one typed actual result | empty |
| 2 | empty | `query-result-adapter: <token>` or `query-result-adapter: <token> <pointer>` |

Tokens are `usage`, `stdin`, `json`, `schema`, `budget`, `decimal`, `int64`, `io`, `protocol`, `completion`, `rowcount`, and `type`. A pointer is a JSON pointer into the stream and does not contain a value from the document. A read failure is `stdin`. Invalid UTF-8 of the whole stream is `json` with no pointer. A line that is not one JSON value, including a duplicate key or trailing bytes on that line, is `json` and the line pointer `/0`, `/1`, and so on. A stdout or stderr write failure is `io`. `usage`, `stdin`, and `io` have no pointer.

There is no exit 1. A difference from an expected result is the comparator's mismatch, not an adapter refusal.

## Stream

JSONL. Every record line ends with `\n`. The stream ends on the newline of the last record. A missing final newline, a blank line, or bytes after the summary newline is `protocol`. End of file is not a summary and is not a successful completion.

The first line is the question header. It is not a `query-pg` record. `query-pg` stdout, from `contracts/gateway/query-pg-cli-v1.md`, is row lines and then one summary line. The caller puts the header in front of that stdout. A stream whose first line is a row or a summary is `protocol`. A second header is `protocol`.

`oo_core::parse_json_value` rejects a line larger than `DOCUMENT_MAX_BYTES` (1_048_576), depth past 32, more than 50_000 nodes, a duplicate key, and trailing bytes. JSON null is rejected except where this document allows it. Unknown keys are rejected.

### Header

| Key | Value |
| --- | --- |
| `record` | `header` |
| `version` | `oo.benchmark.query-result-adapter/v1` |
| `turn` | JSON integer 0 through 7 |
| `max_rows` | JSON integer 0 through 2000 |
| `max_result_bytes` | JSON integer 0 through 262144 |
| `columns` | array of 0 through 64 bindings |

A binding has `name`, `type`, and `ordinal`, and no other key. `type` is `bool`, `int64`, `decimal`, or `text`. `name` is 1 to 256 bytes, is not empty, and contains no NUL, no ASCII control, and no `/`. Names are unique by raw bytes. Column order in this array is the output order.

`ordinal` is a JSON integer or JSON null. Null means the question column is not in the `query-pg` row. Every output row then has `{"presence":"missing"}` there. The integers that are present must be exactly `0 .. width-1`, each once. `width` is the number of non-null ordinals. A gap, a duplicate, or an ordinal past 63 is refused. This is the only way a cell becomes `missing`. A `query-pg` null is not missing.

### Row

`query-pg` row line. Keys are `record` (`row`), `turn`, and `cells`. `turn` must equal the header turn. `cells` length must equal `width`.

A cell's `kind` is `null`, `bool`, `int`, or `text`. `null` has no `value`. `bool` is a JSON boolean. `int` and `text` are JSON strings. Any other kind, including `decimal`, is `protocol`. The gateway does not emit a decimal kind. This adapter does not add one.

### Summary

One summary, and it is the last line. A second summary is `protocol`. A row after the summary is `protocol`. A stream that ends after the header, or after rows, with no summary, is `protocol`. That refusal does not invent a successful end from EOF.

Keys, all required, are the `query-pg` summary keys: `record` (`summary`), `turn`, `status`, `truncated`, `returned_rows`, `result_bytes`, `rollback_confirmed`, `socket_dropped`, `end_audit_ok`, `original_sha256`, `executed_sha256`, `stop`, `sqlstate`.

`turn` equals the header turn. `status` is `succeeded`, `partial`, or `failed`. `truncated`, `rollback_confirmed`, `socket_dropped`, and `end_audit_ok` are JSON booleans. `returned_rows` is a JSON integer 0 through 2000. `result_bytes` is a JSON integer 0 through 262144. Both hashes are 64 lowercase hex digits. `stop` is JSON null or `timeout`, `canceled`, `type_refused`, `database`, or `protocol`. `sqlstate` is JSON null or five ASCII uppercase letters or digits.

The row-line count must equal `returned_rows`. A difference is `rowcount`. `returned_rows` must be at most `max_rows`, and `result_bytes` must be at most `max_result_bytes`. A larger count or byte total is `budget`. Crossing 2000 row lines, 64 columns, or the typed payload cap below is also `budget`.

`truncated`, `stop`, `sqlstate`, `rollback_confirmed`, and `socket_dropped` are checked and then dropped. They do not become warnings. They do not change `status`.

## Cell mapping

The output cell at a binding comes from `cells[ordinal]`, unless `ordinal` is null.

| Binding type | Gateway cell | Typed cell |
| --- | --- | --- |
| any | `null` | `{"presence":"null"}` |
| any | ordinal null | `{"presence":"missing"}` |
| `bool` | `bool` | `{"presence":"value","value":true}` or `false` |
| `int64` | `int` | `{"presence":"value","value":"<canonical i64>"}` |
| `text` | `text` | `{"presence":"value","value":"<string>"}` |
| `decimal` | `int` or `text` | exact decimal object |

A `bool` binding rejects an `int` or `text` cell. An `int64` binding rejects a `text` cell. A `text` binding rejects an `int` cell. That refusal is `type`. The adapter does not infer one gateway kind from another.

`int64` text is the gateway string, unchanged, when it is a canonical `i64`: `0`, or `-` plus digits, or digits, with no leading zero, no `+`, no fraction, and no exponent. `-0` is refused. The extremes `-9223372036854775808` and `9223372036854775807` are accepted. Anything else is `int64`.

`text` is 0 to 500 Unicode scalars and no NUL. It is not trimmed, case-folded, or normalized. A longer text is `budget`.

`decimal` is accepted only when the binding type is `decimal`. An `int` cell must already be a canonical `i64` string and becomes that integer at scale 0. A `text` cell must be an exact decimal literal: optional `-`, an integer part that is `0` or digits without a leading zero, and an optional `.` plus one or more digits. No `+`, no exponent, no spaces. The coefficient and scale come from `oo_core::ExactDecimal::parse_sql`, which keeps the lexical scale. `-0` and `-0.00` are refused rather than rewritten to unsigned zero. A bad spelling is `decimal`.

`unavailable` is not produced. `not_found` is not produced. Zero row lines and `returned_rows` 0 is an empty table: `kind` is `rows` and `rows` is empty.

## Completion

| `status` | `end_audit_ok` | Output `completion` |
| --- | --- | --- |
| `succeeded` | true | `complete` |
| `succeeded` | false | refusal `completion` |
| `partial` | true or false | `partial` |
| `failed` | true or false | `failed` |

`partial` is not written as `complete`. `failed` is not written as `complete`. A `succeeded` summary whose completion audit is false is not written as `complete` and is not rewritten as `failed`. `truncated: true` does not change this table. A finished stream without a summary does not become `complete`.

`warnings` is always an empty array. The gateway stdout has no warning records, and this adapter does not invent codes from `truncated`, `stop`, or `sqlstate`.

## Typed result

Compact JSON, one trailing newline. `kind` is `rows`. `completion` is the table above. `warnings` is `[]`. `columns` follows the header order, each object `name` then `type`. Each row follows the header order. Row order is the `query-pg` row order.

A decimal cell is `{"coefficient":"120","negative":false,"presence":"value","scale":2}`. A bool, int64, or text cell is `{"presence":"value","value":...}`. A null or missing cell is `{"presence":"null"}` or `{"presence":"missing"}`.

Payload bytes use the comparator's sum: column-name lengths, text UTF-8 lengths, int64 spellings, decimal coefficient lengths, and 1 byte for every cell. The cap is 262144. Crossing it is `budget`.

## Hash

The adapter does not hash a row to decide a cell. The crate exposes `sha256_hex`, which calls `oo_core::canonical::hash_bytes`. Tests check that function against the FIPS 180-4 vectors for the empty string, `abc`, and the 448-bit alphabet string.

## What this protocol does not do

It does not compare two results. It does not read gold. It does not treat a partial result as a complete result. It does not treat SQL NULL as a missing column. It does not treat a gateway `int` or `text` as a decimal unless the question binding says the actual type is `decimal`. It does not claim the gateway can return a PostgreSQL `numeric`. It does not claim that a benchmark suite, a 300-question bank, or a release gate has passed.
