# FILE-D04 decimal amendment

Session `eef831aa-dba4-4af2-a78b-541bc9e0a333`. Task a333. Fixed ancestor `63566b60aabdf29a29bfe55c4866edf3c4081cfd`.

This file freezes the E44 / FILE-D04 repair before any product edit. It amends `tools/query-result-export/CONTRACT.md` only. It does not add a plan child. It does not rename `int`. It does not define exact decimal as the text of an `int` cell. Parent files `contracts/gateway/query-pg-cli-v1.md`, `execution/decisions/numeric-query-contract-a327.md`, and `docs/10-security.md` are not edited.

The expected decimal cell is the query cell already frozen by a327 in `execution/decisions/numeric-query-contract-a327.md`:

```json
{"kind":"decimal","negative":false,"coefficient":"123456789012345678","scale":4}
```

a327 owns the producer. This amendment does not change `query-pg`, the numeric decoder, or `oo-core` decimal arithmetic. A unit test may build this JSON. That test is not a compiled `query-pg` receipt. The live chain is a later proof against a delivered a327 stdout and is outside this freeze.

## Cell

`kind` `decimal` is added beside `null`, `bool`, `int`, and `text`. `int` stays the int2/int4/int8 cell. Its `value` stays canonical `i64` text, including `-9223372036854775808`. Kind `numeric` stays refused. A decimal object that uses `value` instead of the three fields below stays refused.

The object has exactly `kind`, `negative`, `coefficient`, and `scale`.

`negative` is a JSON bool. `coefficient` is a JSON string of ASCII digits, length 1 through 8192, with no leading zero except the single character `0`. `scale` is a JSON integer in `-8192..=8192`, with no fraction, no exponent, no `+`, and no `-0`. Every other JSON number in this command stays an unsigned integer. A negative token is accepted by the grammar and rejected unless it is this scale.

The triple is already canonical. The command does not strip zeros and does not clear a sign. `negative: true` with coefficient `0` is a source failure. A longer coefficient or a scale outside that range is a source failure.

Plain text is the `ExactDecimal` display of that triple, and nothing else:

- scale `0` is the coefficient, with a leading `-` when negative
- a positive scale inserts the point `scale` digits from the right, or `0.` and zeros when the coefficient is shorter
- a negative scale writes the coefficient and then `abs(scale)` zeros

`result_bytes` for the cell is the UTF-8 length of that plain text. Null stays 0, `true` stays 4, and `false` stays 5.

## CSV and sidecar

The CSV field is that plain text. No apostrophe is added. A typed minus stays the first character. Quoting stays the existing RFC 4180 rule and does not apply to this plain text, which has no comma, quote, CR, or LF.

The sidecar gains `decimal_count` and `decimal_cells` after `int_cells` and before `csv_sha256`. Each entry is `row`, `column`, `negative`, `coefficient`, `scale`, and `text`. `text` is the plain text. `int_count` and `int_cells` stay. A column kind may be `decimal`. Nulls may mix with that kind. Two different non-null kinds in one column stay a source failure.

`source_sha256`, null cells, empty text, partial completion, formula counts, citation, `http_auth`, and `xlsx` stay as they are.

## Formula

The transformation id stays `apostrophe-prefix-after-leading-whitespace-or-control`.

The leading run that is ignored before `= + - @` now also ignores U+FEFF. It does not ignore any other format character. One gold text cell is U+FEFF followed by `=1+1`. Its CSV field is U+0027, then U+FEFF, then `=1+1`. The file does not begin with a BOM. Typed `int` and typed `decimal` still receive no apostrophe.

## Hand literals

These are the a327 plain-text literals. They are the frozen expected type, not a compiled stdout.

| plain text | negative | coefficient | scale |
| --- | --- | --- | --- |
| `12345678901234.5678` | false | `123456789012345678` | 4 |
| `-12345678901234.5678` | true | `123456789012345678` | 4 |
| `0.0001` | false | `1` | 4 |
| `-0.0001` | true | `1` | 4 |
| `0.0000` | false | `0` | 4 |

Scale `-2` with coefficient `12` and `negative` false is plain text `1200`. That is the same display rule. It is not a PostgreSQL `dscale`.
