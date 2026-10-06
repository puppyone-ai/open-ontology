# enum-history v1

Compile one OOS enum document at an explicit civil business date, then classify an operator-supplied count vector. The contract name is `enum-history/v1`.

This crate reads `oo_core::oos::parse_enum_yaml`. That function parses `EnumFile` and runs `validate_enum`. This crate does not reimplement the schema. It adds the temporal and coverage rules below.

The crate does not scan a database, write `oo_dict`, schedule a sweeper, certify a metric, or authenticate an owner. Those stay later gates. Every success and refusal report sets these flags false:

- `database_used`
- `source_modified`
- `oo_dict_written`
- `metric_certified`
- `owner_authenticated`
- `sweeper_ran`

`owner` and `review.by` are declared labels from the document. They are not an authenticated owner.

The library does not write files. The CLI reads stdin and writes a JSON document to stdout. It does not rewrite the enum YAML, and it does not copy `coverage` or `unexplained_values` into the observed result.

## Dependencies

Direct pins, exact versions from the repository root lock:

| crate | version |
|---|---|
| oo-core | 0.1.0, path `../../crates/oo-core` |
| serde | 1.0.229, feature `derive` |
| serde_json | 1.0.151, feature `arbitrary_precision` |

`oo-core` keeps its own pinned direct dependencies. The child `Cargo.lock` may contain only registry packages whose name, version, source, and checksum already appear in the root lock, plus the path packages `enum-history` and `oo-core`. No root manifest or root lock edit. No `cargo update`. `rust-version` is 1.94.0. Edition 2024. Safe Rust only.

## CLI

The program takes no arguments. One JSON object on stdin. No second JSON value. Unknown fields are rejected.

| condition | exit | outcome |
|---|---|---|
| extra argument, empty stdin, stdin over 2_097_152 bytes, invalid JSON, trailing bytes, unknown field, wrong JSON type for `enum_yaml`, `as_of`, `observations`, `code`, or `count` | 1 | `malformed` |
| compiled report | 0 | `compiled` |
| contract refusal below | 2 | `refused` |

`count` is a JSON integer from 0 through 2^64-1. A fraction, a string, or a negative number is malformed. `threshold`, when present, must be a JSON string. A JSON number is refusal `threshold_not_decimal_string`, so a binary float cannot enter the ratio.

Stderr for exit 1 is `enum-history: malformed <reason>`. Stderr for exit 2 is `enum-history: refused <reason>`. Stdout is always one JSON object.

## Request

```json
{"enum_yaml":"...","as_of":"YYYY-MM-DD","observations":[{"code":"1","count":1}],"threshold":"0.95"}
```

`threshold` may be omitted. The default spelling is `0.95`.

`as_of` is a core `CivilDate`: Gregorian `YYYY-MM-DD`, years 0001 through 9999. The compiler does not read a clock.

`observations` is the operator's pre-aggregated public fixture. It is not a query. At most 1024 entries. `code` must be a core `Label` (1 to 256 characters, no NUL or other C0 controls). The compiler does not trim. `1` and `1 ` are different codes.

Refusal order, first match wins:

1. `invalid_as_of`
2. `threshold_not_decimal_string` when the JSON type is not a string
3. `invalid_threshold` when `ExactDecimal::parse_sql` rejects the spelling, or the value is negative
4. `observation_bound` when there are more than 1024 entries
5. `invalid_observation_code` when a code is not a `Label`
6. `duplicate_observation` when the same code appears twice. Counts are not merged.
7. `core_enum` when `parse_enum_yaml` returns diagnostics, including `validate_enum`
8. `reserved_class`
9. `reversed_interval`
10. `zero_interval`
11. `review_contradicts_status`
12. `inferred_unapproved`
13. `overlap`
14. `count_overflow` when the `u64` sum of counts overflows. No wrapped total is returned.

A refusal has no `rows`, no coverage, and no partial promotion.

## Intervals

Each definition has a half-open interval `[valid_from, valid_to)`.

A missing `valid_from` is −infinity. A missing `valid_to` is +infinity. `as_of` is inside when `valid_from` is missing or `as_of >= valid_from`, and `valid_to` is missing or `as_of < valid_to`. String order of `YYYY-MM-DD` is the chronological order. The start day is inside. The end day is outside.

Both ends present and `valid_from > valid_to` is `reversed_interval`. Both ends present and equal is `zero_interval`. One missing end cannot be zero or reversed.

## Overlap

Definitions with the same `code` overlap when neither interval ends at or before the other starts. Touching endpoints do not overlap: `[2019-01-01, 2024-01-01)` and `[2024-01-01, +infinity)` are adjacent.

Overlap is refused for every status. A dormant future definition and a historical definition are included. The `as_of` date does not excuse an overlap. A gap is allowed. In a gap, before the first start, or after the last end, no definition is active.

## Classes

`undetermined` and `unexplained` are reserved. They are never approved business classes.

`reserved_class` fires when either name is a key of `classes`, or a definition's `class` is reserved and the status is not the matching forced status. After a successful core parse, status `undetermined` already has class `undetermined`, and status `unexplained` already has class `unexplained`. Those two pairs stay non-business. They are not a second refusal.

Status `approved` already requires a class that exists in `classes`. This crate still rejects a reserved name on that path.

## Approval

Core `Review` is `state` (`approved` or `rejected`), optional `by`, and optional `at`. This crate does not add owner fields.

An `approved` definition whose `review.state` is `rejected` is `review_contradicts_status`.

An `approved` definition whose `source` is `inferred` must have `review.state = approved` and `review.by` present. Otherwise `inferred_unapproved`. `review.at` stays optional, matching the core struct and the checked-in enum example. `dictionary_table`, `human`, and a missing source do not need a review object. A present `by` is still only a declared label.

Status `superseded` or `deprecated` is kept with its original status, class, interval, source, evidence, and review. Status alone does not create a business class.

`historical_business_class` is true only when all of these hold:

- status is `superseded` or `deprecated`
- `class` is present, is not reserved, and is a key of `classes`
- the interval passed the reversed and zero checks
- `review.state` is `approved` and `review.by` is present

That row binds as a business class only at an `as_of` inside its interval. Outside that interval it is not the active meaning.

## Active binding

After overlap rejection, each code has at most one definition containing `as_of`. Bindings are one per distinct code, sorted by code.

| active row | disposition | reason |
|---|---|---|
| `approved` and a business class | business | `approved` |
| `superseded` or `deprecated` with `historical_business_class` | business | `historical_approved` |
| `superseded` or `deprecated` without that evidence | unconfirmed | `historical_unapproved` |
| same, but the review evidence is present and the class is not a declared business class | unconfirmed | `class_not_business` |
| `undetermined` | unconfirmed | `undetermined` |
| `unexplained` | unconfirmed | `unexplained` |
| no row contains `as_of` | unconfirmed | `no_active` |

`no_active` covers a gap, an ended interval, and a not-yet-started interval. The warning kind for that binding is `expired_no_active`.

## Observations and coverage

An observation whose code appears on no definition is unconfirmed. Its warning kind is `unexplained` and its reason is `unknown_code`. It is not inserted into the enum history.

`observed_rows` is the checked sum of counts. `business_rows` is the checked sum of counts whose code's active disposition is business. `unconfirmed_rows` is the remainder. `observation_entries` is the vector length, not the sum.

When `observed_rows` is 0, including an empty vector and a vector of zeros, coverage is undefined. `defined` is false. `numerator`, `denominator`, and `ratio` are null. The result is not 1, `1/1`, or `0/0`. `threshold_met` and `comparison` are null. `review_needed` is true.

Otherwise `numerator` and `denominator` are the decimal digit strings of `business_rows` and `observed_rows`, unreduced. `ratio` is `numerator/denominator`. No `f64` is used.

The threshold string is parsed with `ExactDecimal::parse_sql`. Its fraction keeps the lexical scale: `0.95` is `95/100`, and `0.950` is `950/1000`. Comparison is integer cross-multiplication of those digit strings. `threshold_met` is true when the ratio is equal or above. `comparison` is `below`, `equal`, or `above`.

`review_needed` is true when coverage is undefined, the comparison is `below`, or any warning exists. Equal to the threshold meets it. Warnings still require review. An active non-business binding produces a warning even when that code was not observed; its count is `0`.

`declared_coverage` is the parsed decimal's plain spelling, or null. `declared_unexplained_values` keeps the document order. Neither is an observation, and neither is written back.

## Report order

Success fields, in this order: `contract`, `outcome`, `database_used`, `source_modified`, `oo_dict_written`, `metric_certified`, `owner_authenticated`, `sweeper_ran`, `schema_version`, `enum_name`, `as_of`, `declared_owner`, `declared_coverage`, `declared_unexplained_values`, `threshold`, `rows`, `bindings`, `warnings`, `counts`, `observed_coverage`, `threshold_met`, `comparison`, `review_needed`.

`rows` sort by code, then `valid_from` with missing first, then `valid_to` with missing last, then `source_index`. `warnings` sort by code, kind, reason, then `source_index` with missing last. `counts.by_code` sorts by code.

`rows` include every definition. `source_index` is the original position. Label, class, status, source, evidence, review, and both bounds are copied. `business_class` is set on an eligible approved row and on a row with `historical_business_class`. It is null otherwise.

## Limits

The observation cap is 1024. The stdin cap is 2_097_152 bytes. The enum document remains under the core byte budget; a larger document is `core_enum`. Count arithmetic uses checked `u64` addition. A journal, a database sweeper, hourly scheduling, `oo_dict` writes, metric certification, and customer owner proof are out of this contract.
