# business-calendar v1

Resolve one validated OOS calendar at one explicit `as_of` instant.
The crate reads `oo_core` `CalendarFile`, `parse_calendar_yaml`, `validate_calendar`, and `parse_anchor_expr`.
`oo_core` checks anchor expressions and does not execute them.
This crate evaluates only expressions that checker accepts.
It does not shell out, eval a string as code, or build SQL.

This is not a database materialization, not an owner SQL view, not an authenticator, and not a finance approval.
`database_used`, `finance_approval`, `authenticated`, and `clock_used` are false on every successful report.
A closure label and record hash are caller-declared provenance.
The crate does not verify that hash against a ledger and does not treat the label as an approval.

`schema_version` is `oos/v1`, carried inside `calendar_yaml`.
The stdin contract name is `oo.business-calendar/v1`.

Pinned from the repository root lock. This task does not edit that lock or the root manifest.

| crate | version |
|---|---|
| oo-core | path `../../crates/oo-core`, `=0.1.0` |
| chrono | `=0.4.45`, default features off, `std` on, `clock` off |
| chrono-tz | `=0.10.4`, default features off, `std` on |
| serde | `=1.0.229` with `derive` |
| serde_json | `=1.0.151` with `arbitrary_precision` |
| sha2 | `=0.10.9`, default features off, `std` on |

`rust-version` is 1.94.0. Edition 2024. Safe Rust only.
The standalone `Cargo.lock` is a registry subset of the root lock: same version, source, and checksum.
Path packages are only `business-calendar` and `oo-core`.

## Clock

`as_of` is an explicit RFC 3339 instant with a numeric offset or `Z`.
`oo_core::rfc3339::Rfc3339Timestamp` normalizes it.
Zero offsets (`Z`, `z`, `+00:00`, `-00:00`) become `Z`.
Any other offset keeps `±HH:MM` or `±HH:MM:SS`.
The wall-clock fields are not rewritten into `business_tz`.
The report field `as_of` is that normalized spelling.

There is no `SystemTime`, no `Local::now`, and no `Utc::now`.
A missing configured anchor is a refusal.
The crate does not invent `month_start` from a natural month when that name is absent from `anchors`.
It does not invent a week anchor. `week_start` is copied into the report as metadata.
Core `AnchorName` has no week anchor, and this crate does not add one.

## Local today

`business_tz` is the calendar's IANA name, resolved with the pinned `chrono-tz` table.
`oo_core::tz::TZDB_VERSION` and `chrono_tz::IANA_TZDB_VERSION` must be equal.
A pattern that `TimezoneName` accepts but chrono-tz does not know is `zone_unknown`.
There is no UTC fallback.

The absolute instant is projected into that zone.
`today` is the local civil date of that instant.
An instant just west of local midnight stays on the previous civil date even when the UTC date is already the next day.

## Anchors

Configured names are derived in the order written in `anchors`.
Duplicates are `duplicate_anchor`.

Civil arithmetic uses Gregorian days, not 86400-second steps and not `date_trunc`.

| anchor | value |
|---|---|
| `today` | local civil date of `as_of` |
| `month_start` | day 1 of that month |
| `prev_month_start` | day 1 of the previous month |
| `prev_month_end` | the civil day before `month_start` |
| `fiscal_year_start` | day 1 of `fiscal_year_start_month` in the fiscal year that contains `today` |
| `prev_fiscal_year_end` | the civil day before `fiscal_year_start` |
| `quarter_start` | day 1 of the business fiscal quarter that contains `today` |
| `prev_quarter_start` | three civil months before `quarter_start` |
| `prev_quarter_end` | the civil day before `quarter_start` |
| `last_closed_period_end` | the caller-supplied civil date, unchanged |

End anchors are the inclusive previous civil day.
`prev_month_end` on 1 March is 29 February in a leap year and 28 February otherwise.
A derived date outside `0001-01-01` through `9999-12-31` is `date_out_of_range` for the whole request.
The crate does not return a partial anchor set and does not substitute `today`.

### Business fiscal quarter

`quarter_convention` is `business_fiscal_quarter`.

Count civil months from `fiscal_year_start`.
Month offsets 0, 1, 2 are quarter 0; 3, 4, 5 are quarter 1; 6, 7, 8 are quarter 2; 9, 10, 11 are quarter 3.
`quarter_start` is `fiscal_year_start` plus `3 * quarter` civil months, always day 1.

This matches calendar-quarter boundaries only when `fiscal_year_start_month` is 1, 4, 7, or 10.
Docs/05 `date_trunc('quarter', today)` is that calendar quarter.
This helper does not call it and does not use it when the fiscal month is any other value.
Docs/06 supplies `fiscal_year_start_month` as the business rule. That is the rule here.

The fiscal year containing `today` starts on day 1 of `fiscal_year_start_month`.
If `today`'s month is earlier than that month, the start year is the previous civil year.

## Phrases

Every phrase is `[start, end)`.
Core `Bounds` admits only that spelling.
`inclusive_end_civil` is the civil day before `end`.
The crate does not swap a reversed pair and does not clamp a phrase to a month on its own.
The docs sample clamps with `min(...)`. That expression is evaluated, not approximated.

`oo_core::oos::parse_anchor_expr` must accept the text.
The evaluator then uses the same grammar:

```text
expr  = term { ('+' | '-') term }
term  = '(' expr ')' | digits | ('min' | 'max') '(' expr ',' expr ')' | anchor
```

Spaces are the ASCII space. Tabs are not skipped.
`+` and `-` are left-associative and binary. There is no unary minus.
Digits are a non-negative day count.
`min` and `max` require two arguments of the same type.

| operation | result |
|---|---|
| date + days, days + date | date |
| date - days | date |
| date - date | signed day count |
| days + days, days - days | days |
| min/max of two dates or two day counts | the lesser or greater |

A phrase boundary that is a day count is `expression_refused` / `type_mismatch`.
`date + date` and `days - date` are the same refusal.
Overflow and a date outside the civil range are refusals.
An unknown function, including anything other than `min` and `max`, is `unsupported` when it gets past the core checker, and `calendar_invalid` when the checker rejects it.
The crate does not approximate an unsupported expression with a month length or a 30-day month.

Phrase order is `PatternMap` key order, which is the phrase label's ordered string order, not YAML source order.

Comparison phrases have `current` and `previous` spans.
The docs sample `本月至今对比上月同期` uses

```text
min(month_start, prev_month_start + (today - month_start) + 1)
```

`today - month_start` is the civil day count, not a second count.
Adding that count to `prev_month_start` is day arithmetic.
February therefore contributes 28 or 29 days.
`min` with `month_start` stops the previous window at the next month when the previous month is shorter.

## Zone boundaries

Each emitted civil date has a local midnight in `business_tz`.
The authority for unique, gap, or overlap is `oo_core::tz::resolve_local`.
chrono-tz must agree on the same instants.
A disagreement is `zone_disagreement`.
The crate does not move a gap forward or pick one side of an overlap.

| midnight | report |
|---|---|
| one instant | `status` `unique` and one RFC 3339 stamp of that local midnight |
| gap | `status` `nonexistent` |
| overlap | `status` `ambiguous`, `fold0_rfc3339` earlier, `fold1_rfc3339` later |

Zero offsets are spelled `Z`. Other offsets keep `±HH:MM` or `±HH:MM:SS` when the seconds are non-zero.
The stamp is the local midnight with its zone offset, not the same numbers rewritten as UTC.

`next_midnight` is the midnight of the next civil day, not the next day that exists in the zone.
A skipped civil date stays nonexistent.
`civil_day_seconds` is the UTC second difference of those two midnights only when both are unique.
A 23-hour or 25-hour New York civil day is that difference.
If either midnight is not unique, `civil_day_seconds` is null.
The crate does not replace it with 86400.

A phrase interval is `open` when both endpoint midnights are unique.
Otherwise it is `blocked`.
`block_reason` is `nonexistent_midnight` or `ambiguous_midnight`, preferring the start endpoint.
Civil dates are still reported.
Blocking is the explicit interval result. It is not a shifted instant.

## Closure

`last_closed_period_end` in `anchors` requires `closure`.
The object carries `last_closed_period_end`, `label`, and `record_sha256`.
The date is an explicit Gregorian date.
The label is 1 to 64 bytes, ASCII alphanumeric plus `.` `_` `:` `-`, starting with an alphanumeric.
The hash is 64 lowercase hex digits, not a `sha256:` prefix.
Missing closure is `closure_required`.
Closure on a calendar that does not list the anchor is `closure_not_configured`.
The anchor value is the supplied date even when that date is `today` only by coincidence.
The report repeats `finance_approval: false` and `authenticated: false`.

## Hash and identity

`config_sha256` is SHA-256 of the UTF-8 `calendar_yaml` bytes after JSON decoding, lowercase hex, no prefix.
`business_tz` is the calendar string.
`tzdb_version` is the pinned chrono-tz IANA version.
`as_of` is the normalized instant.

## Stdin

One JSON object:

```json
{
  "contract": "oo.business-calendar/v1",
  "calendar_yaml": "schema_version: oos/v1\n...",
  "as_of": "2026-04-01T03:30:00Z",
  "closure": {
    "last_closed_period_end": "2026-03-31",
    "label": "close-2026-03",
    "record_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
  }
}
```

`closure` may be omitted or null when the anchor is not configured.
Unknown fields are rejected.
Trailing ASCII space, tab, CR, and LF are ignored.
A second JSON value is `malformed`.

| bound | bytes | refusal |
|---|---|---|
| stdin | 262144 | `budget` |
| `calendar_yaml` | 196608 | `yaml_budget` |
| `as_of` | 40 | `malformed` |

## CLI

```text
business-calendar
business-calendar --help
```

Stdin is the document. There is no file argument and no clock flag.

Stdout on success is one compact JSON object and one newline.
Stderr is empty. Exit 0.

A refusal writes one compact JSON object and one newline on stdout, and `business-calendar: refused <reason>` on stderr.
The JSON carries `contract`, `outcome` `refused`, and `reason`.
Optional `detail`, `phrase_index`, `anchor`, and `diagnostic_count` are stable tokens or counts.
The YAML body, the `as_of` text, the closure label, the record hash, and diagnostic messages are not copied into either stream.

| exit | reason |
|---|---|
| 0 | resolved |
| 1 | `malformed`, `contract_mismatch` |
| 2 | usage |
| 3 | `budget`, `yaml_budget` |
| 4 | `calendar_invalid`, `zone_unknown` |
| 5 | `duplicate_anchor`, `closure_required`, `closure_not_configured`, `closure_invalid`, `expression_refused`, `date_out_of_range`, `zone_disagreement`, `as_of_unrepresentable`, `tzdb_disagreement` |

`--help` exits 0 and writes the help text to stdout.
Any other argument exits 2 with stderr `business-calendar: usage` and empty stdout.

## Out of scope

No PostgreSQL view, no `_oo.calendar_config` write, no restricted owner grant, no authentication, no real finance audit, and no claim that SP08-W01 or SEM-D01 is accepted.
Enum history is a different crate.
