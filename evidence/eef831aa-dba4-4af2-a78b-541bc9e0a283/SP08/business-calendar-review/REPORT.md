# business-calendar independent review

Verdict: **APPROVE_LIMITED** for the frozen helper `tools/business-calendar` at `ec8b3953cdcb63035d64fbeca3310214aab002ef`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a283`. This directory is the independent review record. The author session `eef831aa-dba4-4af2-a78b-541bc9e0a269` and its `linux/review-1` are the same author's run. They are not this review. No product file, root lock, spec, or ledger was edited. This file does not accept parent SEM-D01, does not accept an `oo_dict` calendar view, does not accept authenticated finance close, does not accept database materialization, does not close SP08, and does not certify a release.

`finance_approval`, `authenticated`, `database_used`, and `clock_used` stay false on the outputs exercised here. `week_start` is copied metadata. The caller closure hash is echoed and is not checked against a ledger.

## Decisive runs

`linux/proof-3` is one container, `oo-a283-proof3`, id `ab6307b5f59e9d6943c82937bb508d0e65310fd49ef130ada251d7c9fae93641`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a283-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`. `capacity.txt` recorded `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0`. Debian 13. Kernel line `Linux ab6307b5f59e 6.12.76-linuxkit #1 SMP Fri Apr 17 14:56:37 UTC 2026 aarch64 GNU/Linux`. `repo-head.txt` is `c551afea55d5a9164760878a5d6a76835b869875`.

`docker-run.exit` is 0. `version.exit` 0, `blob-cmp.exit` 0, `overlay-absent.exit` 0, `lock.exit` `present`, `lock-compile.exit` 0, `lock-subset.exit` 0, `lock-compare.exit` 0, `lock-summary.exit` 0. `fetch-offline.exit` 0 and `fetch.exit` 0 on the private cargo volume filled by `linux/proof`. `example-calendar.exit` `present`, `example-calendar-unchanged.exit` 0, blob `7a41e6276a17690c2a9c2a03b7d2e63f860f6587`, SHA-256 `2ad3845ec287c24391168ce1e7271d743202b0b2d5cef7d6ad05537e02fcc378`. `smoke-test.exit` 0 before the overlay was copied, `smoke-count.txt` 21, `smoke-names.exit` 0. `overlay-present.exit` 0, `test.exit` 0. `clippy-component.exit` 0, `clippy.exit` 0, `clippy.stdout` empty, Clippy stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 1.19s`. `product-unchanged.exit` 0. `cargo-update-invoked.txt` is `no`. `python-present.flag` is 1. The scripts did not invoke Python.

`linux/proof` is the preserved first attempt, container `oo-a283-proof1`, id `c1cec285c0fcdadb29f91ef2ce7b223a76270a4328031d6e119805b824bdf1b0`. ExitCode 101. The archive omitted `examples/schema/calendar.yaml`. The author smoke stopped in `include_str!` before any test ran. Lock subset exited 0. Offline fetch exited 101 (`no matching package named chrono-tz`). Online `cargo fetch --locked` exited 0.

`linux/proof-2` is the preserved second attempt, container `oo-a283-proof2`, id `c488c0602694a27b1549145dd8372d5f1831897ed4fe3850f0b7bca6e9957c8a`. ExitCode 101. Author smoke exited 0 with 21 passed. Offline fetch exited 0. The overlay exited 101 on `dsl_supported_or_refused_and_week_is_not_an_anchor` with `calendar_invalid`. Clippy exited 101 on `needless_lifetimes` in that overlay. The failing source is `linux/proof-2/failed-a283_handcomputed.rs`. Product bytes were unchanged.

## Author smoke, before the overlay

`cargo test --manifest-path tools/business-calendar/Cargo.toml --locked --offline -- --test-threads=1` on the sealed archive, with the review overlay absent. `linux/proof-3/smoke-test.stdout`:

- library tests: 8 passed
- binary unit tests: 0
- `cli`: 5 passed
- `handcomputed`: 8 passed
- doctests: 0

The passed fields sum to 21. Compile line in `smoke-test.stderr`: `Finished test profile [unoptimized + debuginfo] target(s) in 5.21s`. Names: `unix_day_counts`, `roundtrip_and_edges`, `fiscal_quarters_are_not_calendar_quarters`, `exit_table`, `handwritten_instants`, `month_to_date_clamp`, `day_count_is_not_a_date_and_max_is_later`, `known_answers`, `resolved_stdout_is_exact`, `help_and_usage`, `malformed_contract_and_second_value`, `bounds_and_redaction`, `closure_and_expression_exits`, `example_calendar_shanghai_april`, `fiscal_february_is_not_the_calendar_quarter`, `fiscal_april_and_year_boundary_and_leap_day`, `month_to_date_clamp_and_day_arithmetic`, `zone_date_split_and_dst_lengths`, `missing_and_ambiguous_midnights_stay_unshifted`, `refusals_keep_provenance_and_do_not_substitute`, `source_does_not_read_a_clock`.

`linux/proof-2` recorded the same 21 before its overlay failure. `linux/proof` did not reach those tests.

## Independent overlay

`cargo test --manifest-path tools/business-calendar/Cargo.toml --locked --offline --test a283_handcomputed -- --test-threads=1`. `linux/proof-3/test.stdout`: `ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.28s`.

Names: `compiled_cli_closure_bounds_and_redaction`, `compiled_cli_exact_honolulu_json`, `compiled_cli_new_york_sao_paulo_paris_apia`, `dsl_supported_or_refused_and_week_is_not_an_anchor`, `explicit_as_of_crosses_tokyo_and_los_angeles_midnight`, `fiscal_april_lists_every_core_anchor`, `fiscal_february_is_not_the_calendar_quarter`, `leap_year_boundary_and_month_to_date_clamp`, `missing_closure_is_not_today_and_hash_is_not_approval`.

The YAML strings are inline. Dates used here are 2011-12-29, 2011-12-30, 2011-12-31, 2014-11-30, 2014-12-01, 2014-12-31, 2015-01-01, 2015-03-31, 2015-04-01, 2015-05-17, 2016-01-01, 2016-02-01, 2016-02-10, 2016-02-11, 2016-02-29, 2016-03-01, 2016-03-10, 2016-03-11, 2016-03-31, 2017-01-31, 2017-02-01, 2017-02-28, 2017-03-01, 2017-04-01, 2017-08-01, 2017-10-01, 2017-10-15, 2017-10-16, 2017-10-31, 2017-11-01, 2017-12-31, 2018-01-01, 2018-01-20, 2018-01-31, 2018-02-01, 2018-02-09, 2018-02-10, 2018-02-28, 2018-03-01, 2018-03-31, 2018-04-01, 2018-04-30, 2018-05-01, 2018-05-11, 2018-05-22, 2018-05-31, 2018-06-01, 2018-06-03, 2018-06-04, 2018-06-19, 2018-06-20, 2018-06-21, 2018-06-30, 2019-03-10, 2019-03-11, 2019-11-03, 2019-11-04, and 1976-09-25, 1976-09-26, 1976-09-27.

Hand results that passed:

- Fiscal April, `2018-06-20T11:00:00Z`, `week_start` saturday, all ten configured anchors. `fiscal_year_start` `2018-04-01`, `prev_fiscal_year_end` `2018-03-31`, `quarter_start` `2018-04-01`, `prev_quarter_start` `2018-01-01`, `prev_quarter_end` `2018-03-31`, `month_start` `2018-06-01`, `prev_month_start` `2018-05-01`, `prev_month_end` `2018-05-31`, `today` `2018-06-20`, `last_closed_period_end` `2017-12-31`. UTC midnights end `T00:00:00Z`. Civil day length `86400`. A calendar that configures only `quarter_start` emits that one anchor.
- Fiscal February, `2018-06-20T11:00:00Z`. `quarter_start` `2018-05-01`, `prev_quarter_start` `2018-02-01`, `prev_quarter_end` `2018-04-30`, `fiscal_year_start` `2018-02-01`, `prev_fiscal_year_end` `2018-01-31`. On `2018-01-20T11:00:00Z`, `fiscal_year_start` `2017-02-01`, `prev_fiscal_year_end` `2017-01-31`, `quarter_start` `2017-11-01`, `prev_quarter_start` `2017-08-01`, `prev_quarter_end` `2017-10-31`.
- `2016-03-01` `prev_month_end` is `2016-02-29`. `2017-03-01` is `2017-02-28`. `2015-01-01` has `prev_month_start` `2014-12-01` and `prev_month_end` `2014-12-31`. Leap-day `2016-02-29` with fiscal month 4 has `fiscal_year_start` `2015-04-01`, `quarter_start` `2016-01-01`, `prev_fiscal_year_end` `2015-03-31`.
- Month-to-date `min(month_start, prev_month_start + (today - month_start) + 1)`. On `2018-03-31` the previous exclusive end clamps to `2018-03-01` (inclusive `2018-02-28`) and the current exclusive end is `2018-04-01`. On `2016-03-31` the inclusive previous end is `2016-02-29`. On `2016-03-10` the previous exclusive end stays `2016-02-11` (inclusive `2016-02-10`) and the current exclusive end is `2016-03-11`.
- `Asia/Tokyo` `2018-06-20T14:30:00Z` is civil `2018-06-20`. `2018-06-20T15:30:00Z` is civil `2018-06-21`, midnight `2018-06-21T00:00:00+09:00`. `America/Los_Angeles` `2018-06-21T06:30:00Z` is civil `2018-06-20`, midnight `2018-06-20T00:00:00-07:00`.
- DSL on `2018-06-20` UTC, labels in BTree order `alpha`, `empty`, `near`, `zeta`. `alpha` is `[2018-05-22, 2018-06-21)`. `empty` `max(month_start, today - 40)` is `[2018-06-01, 2018-06-01)` with inclusive end `2018-05-31`, interval open. `near` `min(max(today, month_start), month_start + 3)` is `[2018-06-01, 2018-06-04)`. `zeta` `today + 10` .. `today` stays `[2018-06-30, 2018-06-20)` and is not swapped. `today - month_start` refuses `expression_refused` / `type_mismatch` at phrase index 0. `floor(today)` refuses `calendar_invalid` with `diagnostic_count` greater than 0. Anchor name `week` refuses `calendar_invalid`. A phrase that uses `today` when only `month_start` is configured refuses `calendar_invalid`. `week_start` saturday is metadata.
- Missing closure on a calendar that configures `last_closed_period_end`, at `2015-05-17T04:00:00Z`, refuses `closure_required`. The refusal JSON has no anchor and does not contain `2015-05-17` or `today`. With caller date `2014-11-30`, label `finance-close-2014`, and hash `cafebabe` repeated to 64 hex digits, the report echoes that hash, keeps `today` at `2015-05-17`, and keeps the four flags false. The hash differs from SHA-256 of the label and from SHA-256 of the YAML. A closure on a calendar that does not configure `last_closed_period_end` refuses `closure_not_configured`.

Compiled CLI results that passed:

- Honolulu YAML, SHA-256 `0f47e4d306986fc5a77318f01eb204a3874f3e86e65c1f63cca3e9b3b32839e1`, `as_of` `2018-02-10T08:30:00Z`. Stdout is the one compact JSON object in the test, 17 keys, exit 0, empty stderr. Local today is `2018-02-09`, `fiscal_year_start` `2017-04-01`, offsets `-10:00`, `week_start` `wednesday`, civil day `86400`, closure null, phrases empty. Empty SHA-256 matches `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
- `America/New_York` `2019-03-10T16:00:00Z`: midnights `2019-03-10T00:00:00-05:00` and `2019-03-11T00:00:00-04:00`, `82800` seconds. `2019-11-03T16:00:00Z`: `2019-11-03T00:00:00-04:00` and `2019-11-04T00:00:00-05:00`, `90000` seconds.
- `America/Sao_Paulo` `2017-10-15T15:00:00Z`: today midnight `{"status":"nonexistent"}`, `civil_day_seconds` null, next midnight `2017-10-16T00:00:00-02:00`, `month_start` `2017-10-01T00:00:00-03:00`. The phrase is blocked `nonexistent_midnight`. Stdout does not contain `T01:00:00`.
- `Europe/Paris` `1976-09-26T22:30:00Z`: today midnight ambiguous, fold0 `1976-09-26T00:00:00+02:00`, fold1 `1976-09-26T00:00:00+01:00`, next `1976-09-27T00:00:00+01:00`. Phrase start `1976-09-25T00:00:00+02:00` is unique. The phrase is blocked `ambiguous_midnight`. Stdout does not contain `1976-09-26T00:00:00Z`.
- `Pacific/Apia` `2011-12-31T08:00:00Z`: today midnight `2011-12-31T00:00:00+14:00`. Phrase `gap` starts on nonexistent `2011-12-30` and is blocked. Inclusive end stays `2011-12-30`. Phrase `span` starts `2011-12-29T00:00:00-10:00`, ends `2011-12-31`, stays open, and still reports inclusive end `2011-12-30`. Stdout does not contain `2011-12-30T`.
- Missing closure: exit 5, stderr `business-calendar: refused closure_required`, stdout the three-field refusal, no `2015-05-17`. Success with the caller hash exits 0, flags false, anchor civil `2014-11-30`. Uppercase hash exits 5, `closure_invalid`, detail `hash`. `DEADBEEF` and the label are absent from stdout and stderr.
- Stdin of `STDIN_MAX_BYTES + 1` exits 3 `budget` and does not echo `QQQ`. YAML of `CALENDAR_YAML_MAX_BYTES + 1` exits 3 `yaml_budget` and does not echo `qqq`. `as_of` of 41 `x` characters exits 1 `malformed` and is not echoed. Unknown field `SENTINEL_FIELD_9c2e` exits 1. A second JSON value exits 1 and does not echo `Honolulu`. Phrase `floor(today)` with label `SENTINEL_PHRASE_4b7d` exits 4 `calendar_invalid`, `diagnostic_count` greater than 0, no anchors, and neither sentinel nor `floor` is echoed. Anchor name `week` exits 4 and stdout has no `"name":"week"`.

## Registry triples

`linux/proof-3/lock-subset.stdout` lists 115 registry triples and `summary registry_triples=115 missing=0 mismatch=0 path_packages=2 direct_pins_present=true`. `lock-compare.exit` 0 against `oracle/registry-triples.txt`. Direct pins chrono `0.4.45`, chrono-tz `0.10.4`, serde `1.0.229`, serde_core `1.0.229`, serde_derive `1.0.229`, serde_json `1.0.151`, and sha2 `0.10.9` are in that list. Path packages are `business-calendar 0.1.0` and `oo-core 0.1.0`. Source for each registry package is `registry+https://github.com/rust-lang/crates.io-index`.

Child `Cargo.lock` SHA-256 `7a9470817fc50ce83b44faacd4cd6330b3ee6d76d22a0e0bf9cbb75532f83cf5`. Root `Cargo.lock` SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. The proof copied the child lock into `linux/proof-3/Cargo.lock` and did not copy it back onto the product tree. `src-before.txt` matches `src-after.txt`.

## Archive

Reviewed commit and fixed archive are the same commit, `ec8b3953cdcb63035d64fbeca3310214aab002ef`, tree `00560beb14aba36ba62d944dff409c9fcd75af4d`. `linux/proof-3` archives `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `tools/business-calendar`, and `examples/schema/calendar.yaml`. `.cargo/config.toml` at that commit is an xtask alias and is not in the archive. Source blobs are `git archive` of `tools/business-calendar` at that commit. `blob-cmp.exit` is 0.

| file | sha256 |
|---|---|
| `.gitignore` | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` |
| `CONTRACT.md` | `6c20caef62fc46216d116e550858bbd98471ffb4b3fe7056f5a7234565d61095` |
| `Cargo.toml` | `8a4a6f5cfd7705f74a4d543e295c4d799be5faeabf7d269832e7b410c7923190` |
| `Cargo.lock` | `7a9470817fc50ce83b44faacd4cd6330b3ee6d76d22a0e0bf9cbb75532f83cf5` |
| `src/civil.rs` | `d4817ecf33344df4051f7e0edae119a46a77b437fa6b25dc2ff41dd7f13f84ca` |
| `src/expr.rs` | `a242900ce28f9ef3b3b6e7e7cdf2bfc344f8de8d389cdaddb35b4ad65f6a1d7f` |
| `src/fault.rs` | `9838917bbbd702e9f2f0830d26ad37221e543f47216ab5c980b94f07e5834b9f` |
| `src/lib.rs` | `39d5dd6d5f88d9077d2b014067101f41e59e23351d78c8579c8b345dddfef2b3` |
| `src/main.rs` | `586f8cc619e4ad75887ffe4302ec2de72dd717cd11f86447d0e63df17d770255` |
| `src/sha256.rs` | `898c967c4e40a929f0b57b4d50d6b7759edff5aeb90418644770d8d2aa0d2898` |
| `src/zone.rs` | `3c9ed877788a8c3b91bc0422d863b0f2ed3c75b3dde286192ecc76f43f308693` |
| `tests/cli.rs` | `d4d6ebfb3534f0fc55e01bc9d4f2a1b6722c49a3f8e40c7e50668f3e3172098d` |
| `tests/handcomputed.rs` | `eb835e6d8c2017e45ffd907ed5f8afbd0d276506138bb240f8f8a71f4d029179` |

## Budget

Review builders: `linux/proof`, `linux/proof-2`, `linux/proof-3`, then the manifest pair in `linux/manifest/launch.sh`. One builder at a time, 6 GiB, 1 CPU, 0 PostgreSQL. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Private cargo volume, target volume, and network for the proof containers. The manifest uses network `none` and no cargo volume. `CLEANUP.txt` records removal of this session's network, volumes, and tmp. The pinned image stays. `docker prune` was not run.

## Limits of this approval

The approval is the frozen helper at the commit above: configured core anchors, the anchor expression grammar `oo_core` accepts, zone midnights, and the CLI refusal exits exercised here. `week_start` stays metadata. A missing configured anchor refuses. An expression the core checker rejects is `calendar_invalid`. An expression the checker accepts and the evaluator cannot type is `expression_refused`. The Apia span whose endpoint midnights are unique stayed open while its inclusive civil end remained the skipped day `2011-12-30`, and the stdout contained no shifted `2011-12-30T` stamp.

Parent SEM-D01, an `oo_dict` calendar view, a week anchor, authenticated finance close, database materialization, and a release are outside this child.
