# enum-history independent review

Verdict: **APPROVE_LIMITED** for the enum compiler and CLI in `tools/enum-history` at `e76813f62150094f1b5b137d0e9d3ce291580149`, built from the fixed archive `7f500ff0a61a42ea606a281dd931099091a52ef6`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a278`. This directory is the independent review record. The author session `eef831aa-dba4-4af2-a78b-541bc9e0a270` and its `linux/review-1` are the same author's run. They are not this review. No product file, root lock, spec, or ledger was edited. This file does not accept parent SEM-D02, does not close SP08, and does not certify a release.

The crate compiles one OOS enum at an explicit civil date and classifies a caller-supplied count vector. `database_used`, `source_modified`, `oo_dict_written`, `metric_certified`, `owner_authenticated`, and `sweeper_ran` stay false on the outputs exercised here. `owner` and `review.by` stay declared labels.

## Decisive runs

`linux/proof-2` is one container, `oo-a278-proof2`, id `0a15a41b59667a15f7f0d2378b5753600a121b9eb4f70529602cb323e168303a`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a278-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`. `capacity.txt` recorded `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0`. Debian 13. `repo-head.txt` is `aed25ac3651a9da8fc1839daaf582464a0935024`.

`docker-run.exit` is 0. `version.exit` 0, `blob-cmp.exit` 0, `overlay-absent.exit` 0, `lock.exit` `present`, `lock-compile.exit` 0, `lock-subset.exit` 0, `lock-compare.exit` 0, `lock-summary.exit` 0. `fetch-offline.exit` 0 and `fetch.exit` 0 on the private cargo volume filled by `linux/proof`. `smoke-test.exit` 0 before the overlay was copied, `smoke-count.txt` 23, `smoke-names.exit` 0. `overlay-present.exit` 0, `test.exit` 0. `clippy-component.exit` 0, `clippy.exit` 0, `clippy.stdout` empty, Clippy stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 1.00s`. `product-unchanged.exit` 0. `cargo-update-invoked.txt` is `no`.

`linux/proof` is the preserved first attempt, container `oo-a278-proof1`, id `8be6711282c90175cdb4c48a86ed10f30013c4af316635afab0188bbef91e8f0`. ExitCode 101, OOMKilled false, Privileged false, same network, CapDrop, memory, and CPU. Its author smoke, lock subset, and Clippy exited 0. The overlay exited 101. The failing source is `linux/proof/failed-a278_handcomputed.rs`.

## Author smoke, before the overlay

`cargo test --manifest-path tools/enum-history/Cargo.toml --locked --offline -- --test-threads=1` on the sealed archive, with the review overlay absent. `linux/proof-2/smoke-test.stdout`:

- lib ratio tests: 3 passed
- bin: 0 tests
- `cli`: 2 passed
- `handcomputed`: 18 passed
- doctests: 0

The passed fields sum to 23. Compile line in `smoke-test.stderr`: `Finished test profile [unoptimized + debuginfo] target(s) in 4.38s`. The handcomputed batch finished in 0.12s. Names: `nineteen_twentieths_equals_lexical_0_95`, `trailing_zero_keeps_its_denominator`, `one_hundred_over_one_hundred_five_is_above_0_95`, `stdin_compiles_and_refuses`, `threshold_number_duplicate_and_malformed_exits`, `zero_rows_are_undefined_and_not_one`, `full_business_coverage_needs_no_review`, `boundary_includes_start_and_excludes_end`, `gap_keeps_both_rows_and_does_not_promote`, `dormant_future_overlap_and_historical_overlap_refuse`, `reversed_and_zero_intervals_refuse`, `retroactive_correction_preserves_history_and_declared_coverage`, `superseded_without_review_is_not_promoted`, `deprecated_with_review_keeps_status_and_historical_class`, `reserved_names_never_become_business_classes`, `inferred_approval_requires_core_review_fields`, `unknown_code_is_unexplained_and_exact_threshold_matches`, `active_undetermined_warns_even_when_unobserved`, `class_not_in_the_map_is_not_historical_business`, `duplicate_observation_and_overflow_refuse_without_a_report`, `observation_bound_precedes_core_parse`, `bad_date_and_threshold_and_core_yaml_refuse`, `canonical_enum_keeps_declared_coverage_separate`.

`linux/proof` recorded the same 23 before its overlay failure.

## Independent overlay

`cargo test --manifest-path tools/enum-history/Cargo.toml --locked --offline --test a278_handcomputed -- --test-threads=1`. `linux/proof-2/test.stdout`: `ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s`.

Names: `adjacent_half_open_keeps_history_and_declared_coverage`, `compiled_cli_reports_history_and_refuses_overlap`, `exact_ninety_five_of_one_hundred`, `future_and_historical_overlaps_refuse`, `gap_day_binds_no_active_and_keeps_both_rows`, `inferred_approved_needs_explicit_review`, `overflow_refuses_without_a_wrapped_total`, `reserved_classes_are_not_business`, `superseded_without_approval_stays_unconfirmed`, `unknown_zero_count_is_not_inserted`, `zero_observations_are_undefined`.

The YAML strings are inline. They do not `include_str!` the author fixtures. Dates used here are 2009-03-01, 2009-07-01, 2009-12-01, 2010-02-01, 2011-01-01, 2012-01-01, 2012-01-31, 2012-02-01, 2012-08-15, 2013-04-01, 2013-05-01, 2013-08-31, 2013-09-01, 2014-03-02, 2014-06-30, 2014-07-01, 2014-09-01, 2015-01-01, 2028-01-01, 2028-06-01, 2029-01-01, and 2030-01-01.

Hand results that passed:

- Adjacent `[2012-02-01, 2014-07-01)` superseded and `[2014-07-01, +infinity)` approved. On `2014-07-01` the binding is `approved` / `closed` at source index 1, and the superseded row stays, with `historical_business_class` and declared review `treasury.chen`. On `2014-06-30` and on the start day `2012-02-01` the binding is `historical_approved` / `open` at source index 0. On `2012-01-31` both rows stay and the binding is `no_active`, ratio `0/8`. Declared coverage stays `0.40`. Declared unexplained value `z` stays in the list and is not a row. Observed ratio on the end day is `8/8`. `owner_authenticated` stays false.
- Gap `[2011-01-01, 2012-01-01)` and `[2013-05-01, 2013-09-01)`. `2012-08-15` keeps both rows, binds `no_active`, ratio `0/7`, warning `expired_no_active`. `2011-01-01` binds `open` at index 0. `2012-01-01` is outside. `2013-08-31` binds `closed` at index 1. `2013-09-01` is outside.
- Future overlap `[2028-01-01, 2029-01-01)` with `[2028-06-01, 2030-01-01)`, judged at `2014-07-01`, refuses `overlap` with detail `code `k` source_index 0 and 1`. Historical overlap `[2009-03-01, 2009-12-01)` with `[2009-07-01, 2010-02-01)`, same `as_of`, refuses the same way. The refusal JSON has no `rows`, `bindings`, or `observed_coverage`. The six closure flags are false.
- Inferred `approved` with `review.state=approved`, `review.by=treasury.chen`, and `review.at=2014-03-02` binds business class `closed`, ratio `11/11`, `review_needed` false, `owner_authenticated` false. The same row without `review.by` refuses `inferred_unapproved`.
- Superseded `[2013-01-01, 2015-01-01)` with no review stays status `superseded`, `historical_business_class` false, binding `historical_unapproved`, business rows `0`, unconfirmed rows `6`.
- Class-map key `unexplained` refuses `reserved_class`, detail `class `unexplained` is reserved`. An approved row whose class is `undetermined` and whose class map does not contain that name refuses `core_enum`; the detail contains `/values/0/class` and `an approved code's class must exist on this enum`. A deprecated row with class `unexplained` refuses `reserved_class`, detail `source_index 0 uses reserved class `unexplained``. A forced `undetermined` row beside an approved business row stays unconfirmed, business class null, warning count `0`, while the observed ratio is `4/4` and `review_needed` is true.
- Unknown code `q` with count `0` is warning `unexplained` / `unknown_code` / count `0` and is not a history row. Observed rows are `0`, so coverage is undefined: `defined` false, ratio null, `review_needed` true.
- Counts `95` and `5` produce unreduced `95/100`, comparison `equal`, `threshold_met` true, default threshold numerator `95` and denominator `100`. The unknown code is not inserted. The warning makes `review_needed` true. Counts `94` and `6` are `94/100`, comparison `below`, `threshold_met` false.
- An empty vector and a known code with count `0` leave coverage undefined. The ratio is null. It is not `1`, `1/1`, or `0/0`. The default threshold fraction stays `95/100`.
- `u64::MAX` plus `1` on two business codes refuses `count_overflow` with detail `observation counts overflow u64` and no rows. `u64::MAX` alone reports observed rows `18446744073709551615`, comparison `above`, and does not wrap to `0`.

The compiled CLI, `CARGO_BIN_EXE`, on the adjacent document at `2014-07-01` with count `8` exits 0. Stdout is one JSON object. Stderr is empty. Declared coverage is `0.40`, both rows remain, the binding reason is `approved`, and the six flags are false. The future overlap exits 2 with stderr `enum-history: refused overlap` and the same detail. `--help` exits 1 with stderr `enum-history: malformed usage` and outcome `malformed`.

## Registry triples

`linux/proof-2/lock-subset.stdout` lists 115 registry triples and `summary registry_triples=115 missing=0 mismatch=0 path_packages=2 direct_pins_present=true`. `lock-compare.exit` 0 against `oracle/registry-triples.txt`. The crate lock's registry packages are a subset of the root lock in the fixed archive, checksums included. Direct pins serde 1.0.229, serde_core 1.0.229, serde_derive 1.0.229, and serde_json 1.0.151 are in that list. Path packages are `enum-history 0.1.0` and `oo-core 0.1.0`. Source for each registry package is `registry+https://github.com/rust-lang/crates.io-index`.

Child `Cargo.lock` SHA-256 `32ff28450510cd4cc375cc49ed52fe4be3a6a09af3d2a17b80c94c9ee90fa7a8`. Root `Cargo.lock` SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. The proof copied the child lock into `linux/proof-2/Cargo.lock` and did not copy it back onto the product tree. `src-before.txt` matches `src-after.txt`.

## Archive

Reviewed commit `e76813f62150094f1b5b137d0e9d3ce291580149`, tree `a35f3cfdce10bbff42ffd32413bb92bad12a106c`. Fixed archive `7f500ff0a61a42ea606a281dd931099091a52ef6`, tree `5c000b8d8cf492fc99fb897aae39da673b3299fa`. `tools/enum-history` is byte-identical between those commits. The proof archive is `git archive` of the fixed commit for `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, and `tools/enum-history`. Source blobs are `git archive` of the reviewed commit. `blob-cmp.exit` is 0.

| file | sha256 |
|---|---|
| `CONTRACT.md` | `01de18f2d1108bf24fabfd5e8bf3a8d712d89618bdef44eaffa96653bc348b63` |
| `Cargo.toml` | `82272ffb2b95e2b7647ed3f70cec106fb8cdfd2c41d4cad0311f7ef532eb28ae` |
| `Cargo.lock` | `32ff28450510cd4cc375cc49ed52fe4be3a6a09af3d2a17b80c94c9ee90fa7a8` |
| `src/lib.rs` | `f1b979574b4bcd458aa7e61dd9f5ab651f1a94a797ec0dee84c3d2b98e382859` |
| `src/main.rs` | `98e948c4916e92e65fc49cb45b60750b73406e1c5afc32aac92d1d8c6ab56847` |
| `src/ratio.rs` | `d90e37a447739c414e1b3e362e6c22e8f07b4ec37b17d38cde416a60bbd12ad8` |

## Budget

Review builders: `linux/proof`, then `linux/proof-2`, then the manifest pair in `linux/manifest/launch.sh`. One builder at a time, 6 GiB, 1 CPU, 0 PostgreSQL. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Private cargo volume, target volume, and network for the proof containers. The manifest uses network `none` and no cargo volume. `CLEANUP.txt` records removal of `oo-a278-eef831aa-net`, `oo-a278-eef831aa-cargo`, `oo-a278-eef831aa-target`, and `/tmp/oo-a278-eef831aa-dba4-4af2-a78b-541bc9e0a278`. The pinned image stays. `docker prune` was not run. The image contains Python (`python-present.flag` is 1). Python was not invoked.

`linux/proof` offline fetch exited 101 because the fresh cargo volume had no `serde_json`. The same container then fetched online and exited 0. `linux/proof-2` reused that private volume and fetched offline with exit 0.

## Limits of this approval

The count vector is the operator's pre-aggregated fixture. `database_used` stays false. This crate does not scan a database.

`owner` and `review.by` are labels from the document. `owner_authenticated` stays false.

Declared coverage is echoed. Observed coverage is a separate unreduced ratio. A zero observed total is undefined.

Superseded and deprecated rows stay in the history. A historical business class needs the class key plus `review.state=approved` and `review.by`. Status text alone does not promote a row.

`core_enum` is judged before `reserved_class`. An approved class that is absent from the class map is `core_enum`. A reserved name that is a class-map key, and a non-forced status that uses a reserved class, are `reserved_class`.

A database sweeper, hourly scheduling, an `oo_dict` write, metric certification, and customer owner proof stay later gates. This review had no PostgreSQL.
