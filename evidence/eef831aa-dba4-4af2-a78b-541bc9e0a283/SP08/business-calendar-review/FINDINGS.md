# Findings

Reviewer: this session, sole coder and reviewer. This is a separate session from the author of `tools/business-calendar`. It is not parent acceptance. The decisive build is `linux/proof-3`. `linux/proof` and `linux/proof-2` are preserved.

## Verdict

The frozen helper matches `CONTRACT.md` on the runs below. `linux/proof-3` is Linux arm64 Rust 1.94.0. The author suite exited 0 before the overlay (8 library, 5 CLI, 8 hand-computed, 21 total). The independent overlay exited 0 (9 passed). Clippy `--all-targets -D warnings` exited 0. `lock-subset` printed `registry_triples=115 missing=0 mismatch=0 path_packages=2 direct_pins_present=true`. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` was unchanged. Child lock `7a9470817fc50ce83b44faacd4cd6330b3ee6d76d22a0e0bf9cbb75532f83cf5` was unchanged. `cargo-update-invoked=no`. No PostgreSQL container was started.

I read `CONTRACT.md`, the crate sources, `oo_core` `CalendarFile` and `parse_anchor_expr`, the author fixtures, the a269 REPORT, FINDINGS, and HANDOFF, and the proof exits after those containers exited.

## What the runs show

Fiscal April and fiscal February use `business_fiscal_quarter`. On `2018-06-20` with fiscal month 2, `quarter_start` is `2018-05-01`. On `2018-01-20` the fiscal year is still the one that started `2017-02-01`, and `quarter_start` is `2017-11-01`.

Leap day and the year boundary stay civil dates. `2016-02-29` is a real `prev_month_end` and a real `today`. `2015-01-01` reaches `2014-12-01` and `2014-12-31`. Month-to-date clamps the previous exclusive end to `month_start` when the day-count would pass it, including the leap clamp to inclusive `2016-02-29`. A shorter span, `2016-03-10`, does not clamp.

An explicit `as_of` selects the business-zone civil date. Tokyo `2018-06-20T15:30:00Z` is the next civil day. The same UTC calendar day at `14:30:00Z` is still `2018-06-20`. Los Angeles `2018-06-21T06:30:00Z` is still `2018-06-20`.

New York `2019-03-10` is 82800 seconds, offsets `-05:00` then `-04:00`. New York `2019-11-03` is 90000 seconds, offsets `-04:00` then `-05:00`. Sao Paulo `2017-10-15` midnight stays nonexistent, the phrase stays blocked, and the next unique midnight is `2017-10-16T00:00:00-02:00`. Paris `1976-09-26` midnight stays ambiguous with fold0 `+02:00` and fold1 `+01:00`, and the phrase that ends there stays blocked. Apia `2011-12-31` keeps `+14:00`. The phrase that starts on `2011-12-30` is blocked `nonexistent_midnight`. The phrase whose endpoints are `2011-12-29` and `2011-12-31` stays open. Inclusive end remains `2011-12-30`. No `2011-12-30T` stamp is emitted.

Supported expressions evaluate. `min`/`max`, addition, and subtraction produce the civil dates listed in `REPORT.md`. A reversed range is not swapped. An empty range stays open. `today - month_start` is `expression_refused` / `type_mismatch`. `floor(today)` and the anchor name `week` are `calendar_invalid`. `week_start` remains metadata.

A missing closure refuses `closure_required` and does not substitute today. The caller hash is echoed. `finance_approval`, `authenticated`, `database_used`, and `clock_used` stay false. An uppercase hash refuses `closure_invalid` with detail `hash` and is not copied into the streams. Bounds refusals do not echo the oversized stdin, YAML, `as_of`, unknown field, second document, or phrase sentinel.

The compiled CLI Honolulu document is one JSON object, exit 0, empty stderr. The local civil date is `2018-02-09` for `as_of` `2018-02-10T08:30:00Z`.

## Preserved failures

`linux/proof` archived the tool without `examples/schema/calendar.yaml`. `tests/handcomputed.rs` includes that file. Compile exited 101 with `No such file or directory` before the 21 tests ran. Lock subset had already printed the 115-triple summary. Offline fetch exited 101 on the empty cargo volume. Online fetch exited 0. The crate was not edited.

`linux/proof-2` included that sealed file. The author 21 passed. The overlay phrase block used a Rust backslash line continuation, which strips the indent of the next line, so the first phrase key was not under `phrases`. Resolve returned `calendar_invalid`. Clippy then denied `needless_lifetimes` on `range_end` in the overlay. `linux/proof-2/failed-a283_handcomputed.rs` is that source. `test.stdout` records 8 passed and `dsl_supported_or_refused_and_week_is_not_an_anchor` failed. The crate was not edited.

`linux/proof-3` keeps the phrase indent with explicit newlines and elides that lifetime. Those nine assertions passed with Clippy.

## Limits that stay in force

This crate does not read a clock, write `oo_dict`, open a database, or approve a finance close. The closure record hash is caller-declared provenance.

`week_start` is metadata. There is no week anchor in this helper.

The image contains Python. The proof scripts did not invoke it. The registry check and the manifest tool are Rust.

## Not accepted here

Parent SEM-D01, an `oo_dict` calendar view, a week anchor, authenticated finance close, database materialization, SP08 as a whole, and a release are outside this child.
