# Result comparison v1

Protocol id: `oo.benchmark.result-comparison/v1`.

Report id: `oo.benchmark.result-comparison-report/v1`.

This document is the frozen input and output of one comparison. It is a reusable primitive for a typed expected result and a typed actual result. It is not a question bank, a scorer, a judge, a pass^k calculator, a bootstrap, an HTML report, or an acceptance of a quality benchmark.

The crate that implements this document is a standalone workspace. It links `oo-core` from the fixed source archive and uses serde 1.0.229 and serde_json 1.0.151. It does not edit the root manifest or the root lock.

## Command

```text
result-comparator
result-comparator --help
```

No other arguments. `--help` and `-h` write the help text below to stdout, write nothing to stderr, do not read stdin, and exit 0. Any other argument list writes `result-comparator: usage` to stderr, writes nothing to stdout, does not read stdin, and exits 2.

Help text, including the final newline:

```text
result-comparator reads oo.benchmark.result-comparison/v1 from stdin.
It writes oo.benchmark.result-comparison-report/v1 to stdout.
exit 0 match
exit 1 mismatch
exit 2 config
```

With no arguments the process reads stdin until EOF, capped at 1_048_577 bytes. A longer stream is refused before parse. The bytes must be UTF-8. The process writes one compact JSON report and a final newline to stdout when the document is a comparison. Stderr is empty for a match and for a mismatch.

| Exit | Stdout | Stderr |
| --- | --- | --- |
| 0 | report, verdict `match` | empty |
| 1 | report, verdict `mismatch` | empty |
| 2 | empty | `result-comparator: <token>` or `result-comparator: <token> <pointer>` |

Tokens are `usage`, `stdin`, `json`, `schema`, `budget`, `decimal`, `int64`, and `io`. A pointer is a JSON pointer into the input and does not contain a value from the document. A read failure is `stdin`. Invalid UTF-8, broken JSON, a duplicate key, trailing bytes, and the core depth or node budget are `json`. A stdout or stderr write failure is `io`. Those three tokens have no pointer.

A mismatch is only a well-formed pair that differs. An ill-formed document is config, exit 2, and produces no report.

## Document

One JSON value. `oo_core::parse_json_value` rejects a document larger than `DOCUMENT_MAX_BYTES` (1_048_576), depth past 32, more than 50_000 nodes, a duplicate key, and trailing bytes. JSON null is rejected in every field. Unknown keys are rejected. A JSON number is accepted only for `scale`.

Top-level keys, all required:

| Key | Value |
| --- | --- |
| `version` | the string `oo.benchmark.result-comparison/v1` |
| `row_comparator` | `ordered` or `unordered` |
| `expected` | one typed result |
| `actual` | one typed result |

`set` is not a comparator. Ties are not inferred from equal keys. `unordered` is the explicit unordered comparator, and it is a multiset: order is ignored and duplicate rows keep their multiplicity. Dropping a duplicate is a mismatch.

## Typed result

| Key | When |
| --- | --- |
| `kind` | `rows` or `not_found` |
| `completion` | `complete`, `partial`, or `failed` |
| `warnings` | array, possibly empty |
| `columns` | required for `rows`, forbidden for `not_found` |
| `rows` | required for `rows`, forbidden for `not_found` |

`complete` is the caller's name for an executor `succeeded` read. `partial` and `failed` use the executor's words. This program does not read query-pg JSONL, a `truncated` flag, SQL, or an audit file. Empty `rows` is an empty table, not `not_found`.

A warning object has `code` and `message` and no other key. `code` is 1 to 64 bytes matching `^[A-Z][A-Z0-9_]*$`. `message` is 1 to 200 Unicode scalars and contains no NUL. Warning order is significant. Duplicate codes are not collapsed.

A column object has `name` and `type` and no other key. `type` is `bool`, `int64`, `decimal`, or `text`. `name` is 1 to 256 bytes, is not empty, and contains no NUL, no ASCII control, and no `/`. Names are unique by raw bytes. Column order is the row order.

A row is an array whose length is the column count. Each cell is an object:

| Column type | Cell |
| --- | --- |
| any | `{"presence":"null"}`, `{"presence":"missing"}`, or `{"presence":"unavailable"}` |
| `bool` | `{"presence":"value","value":true}` or `false` |
| `int64` | `{"presence":"value","value":"<canonical i64>"}` |
| `text` | `{"presence":"value","value":"<string>"}` |
| `decimal` | `{"presence":"value","negative":false,"coefficient":"120","scale":2}` |

A null, missing, or unavailable cell has no other key. A value cell has no extra key. `null`, `missing`, and `unavailable` are three presences. None of them is another.

`int64` text is a JSON string, never a JSON number. The spelling is `0`, or `-` plus digits, or digits, with no leading zero, no `+`, no fraction, and no exponent. `-0` is refused. The value must fit in `i64`, including `-9223372036854775808` and `9223372036854775807`. A value outside that range is config, not a mismatch.

`text` is a JSON string of 0 to 500 Unicode scalars and no NUL. It is not trimmed or case-folded.

`decimal` uses a boolean `negative`, a string `coefficient`, and an integer `scale`. The coefficient is ASCII digits, 1 to 8192 characters, with no leading zero unless the coefficient is `0`. `negative: true` with coefficient `0` is refused. `scale` is a JSON integer, no fraction and no exponent, whose magnitude is at most 8192. The coefficient is not a JSON number. Nothing in a decimal is an IEEE-754 value.

These limits also apply to each side:

| Limit | Value |
| --- | --- |
| rows | 2000 |
| columns | 64 |
| warnings | 32 |
| payload bytes | 262144 |

Payload bytes are the sum of column-name lengths, warning code lengths, warning message UTF-8 lengths, text UTF-8 lengths, int64 spellings, decimal coefficient lengths, and 1 byte for every cell. The cap is the query-pg result byte cap. Crossing a limit is `budget`, not a partial report. The pointer is `/` for the document, `/expected/rows`, `/expected/columns`, or `/expected/warnings` for those arrays, `/expected` for that side's payload, and the cell or warning field when a single value is too long. The actual side uses `/actual` in the same places. A decimal coefficient or scale that breaks its own rule is `decimal`. An int64 spelling that breaks its rule is `int64`.

## Comparison

The walk order is fixed.

1. `kind`
2. `completion`
3. `warnings`, as one ordered list
4. Stop if either kind is `not_found`.
5. `columns`, as one ordered list of names and types. Stop on a difference.
6. Rows, using `row_comparator`.

`partial` never matches `complete`. `failed` matches only `failed`. A partial actual does not match a complete expected, and a complete actual does not match a partial expected. The same rule applies when `kind` is `not_found`.

Warnings match only when the code and message sequences are identical.

Columns match only when names, types, and order are identical.

`ordered` compares row `i` with row `i`. A length difference records `rows` with reason `length` and the two counts, then compares the shared prefix. A cell records `rows/<row>/<column>`, with indexes from zero.

`unordered` counts canonical row strings. Counts that differ record reason `multiplicity`, the two counts, and the row string. The records are sorted by that row string. Equal counts match, including a row that appears twice on both sides. One copy against two copies does not match.

Cell equality:

| Case | Rule |
| --- | --- |
| presence | the same presence word |
| bool | the same boolean |
| int64 | the same canonical string |
| text | the same UTF-8 bytes |
| decimal | exact equality below |

Decimal equality calls `oo_core::ExactDecimal::rescale_exact` on the left value at the right value's scale. The rescaled value matches when it is equal to the right value. An inexact rescale is a mismatch. No float and no relative tolerance is used. A negative scale is a real scale: coefficient `12` at scale `-2` matches coefficient `1200` at scale `0`.

A coefficient of `0` has no trailing zero that `rescale_exact` can strip without emptying the digit string, so that function cannot move a zero onto another scale. Two non-negative zero coefficients match at every pair of scales. That is exact zero, not a tolerance. A zero does not match a non-zero coefficient.

## Report

Compact JSON, one trailing newline, keys in alphabetical order because the writer builds a JSON object map.

```json
{"differences":[],"verdict":"match","version":"oo.benchmark.result-comparison-report/v1"}
```

`verdict` is `match` only when `differences` is empty. Otherwise `verdict` is `mismatch` and `differences` is non-empty.

Each difference is an object. Keys are alphabetical. `row` is present only for `multiplicity`.

| Reason | `path` | `expected` and `actual` |
| --- | --- | --- |
| `kind` | `kind` | `rows` or `not_found` |
| `completion` | `completion` | `complete`, `partial`, or `failed` |
| `warnings` | `warnings` | canonical warning array |
| `schema` | `columns` | canonical column array |
| `length` | `rows` | row count, decimal digits |
| `presence` | `rows/<r>/<c>` | canonical cell |
| `value` | `rows/<r>/<c>` | canonical cell |
| `decimal` | `rows/<r>/<c>` | canonical cell |
| `multiplicity` | `rows` | count digits, plus `row` |

A canonical cell, row, warning array, or column array is compact JSON with object keys sorted alphabetically. A decimal cell is `{"coefficient":"120","negative":false,"presence":"value","scale":2}`. A bool, int64, or text cell is `{"presence":"value","value":...}`. A presence-only cell is `{"presence":"null"}`.

## Hash

The comparison does not hash a row to decide equality. The crate exposes `sha256_hex`, which calls `oo_core::canonical::hash_bytes`. Tests check that function against the FIPS 180-4 vectors for the empty string, `abc`, and the 448-bit alphabet string. Those digests are published vectors. They are not copied from this crate's report.

## What this protocol does not do

It does not call a model. It does not compute pass^k, a paired test, or a bootstrap. It does not score prose, citations, or customer data. It does not accept a partial result as a complete result. It does not treat a multiset as a set. It does not treat `1.20` and `1.21` as equal. It does not claim that a benchmark suite, a 300-question bank, or a release gate has passed.
