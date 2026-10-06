# Findings

Reviewer: this session, sole coder and reviewer. This is a separate session from the author of `tools/enum-history`. It is not parent acceptance. The decisive build is `linux/proof-2`. `linux/proof` is preserved.

## Verdict

The frozen compiler matches `CONTRACT.md` on the runs below. `linux/proof-2` is Linux arm64 Rust 1.94.0. The author suite exited 0 before the overlay (3 ratio, 2 CLI, 18 hand-computed, 23 total). The independent overlay exited 0 (11 passed). Clippy `--all-targets -D warnings` exited 0. `lock-subset` printed `registry_triples=115 missing=0 mismatch=0 path_packages=2 direct_pins_present=true`. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` was unchanged. Child lock `32ff28450510cd4cc375cc49ed52fe4be3a6a09af3d2a17b80c94c9ee90fa7a8` was unchanged. `cargo-update-invoked=no`. No PostgreSQL container was started.

I read `CONTRACT.md`, `src/lib.rs`, `src/main.rs`, `src/ratio.rs`, the author fixtures, the a270 REPORT, FINDINGS, and HANDOFF, and the proof exits after those containers exited.

## What the runs show

Half-open `[valid_from, valid_to)` holds on dates that are not the author fixture dates. `2014-07-01` is inside a row that starts that day and outside a row that ends that day. A gap on `2012-08-15` keeps both rows and binds `no_active`.

Overlap refuses when `as_of` is inside neither interval. The future pair is entirely after `2014-07-01`. The historical pair is entirely before it. Touching endpoints in the adjacent document stay two rows.

`unexplained` as a class-map key refuses `reserved_class`. A deprecated row whose class is `unexplained` also refuses `reserved_class`. An approved row whose class is `undetermined`, when that name is not a class key, refuses `core_enum` first. A forced undetermined row stays in the history, is not a business class, and warns with count `0` when it was not observed.

An inferred approved row with `review.state=approved` and `review.by` binds a business class. `review.at` is carried. `owner_authenticated` stays false. Missing `review.by` refuses `inferred_unapproved`.

A superseded row with no review stays `superseded` and `historical_unapproved`. It contributes no business count. Declared coverage `0.40` and declared unexplained value `z` are echoed. The caller YAML string is unchanged. Observed ratio `8/8` is a different field.

Unknown code `q` with count `0` is `unknown_code` and is not inserted. Empty observations and an all-zero known count leave coverage undefined. `95/100` equals the default `0.95` and still needs review because of the unknown warning. `94/100` is below. `u64::MAX + 1` refuses `count_overflow` and returns no wrapped total. `u64::MAX` alone stays the decimal `18446744073709551615`.

The compiled CLI exits 0, 2, and 1 for the compiled document, the overlap refusal, and `--help`. Stdout is one JSON object. The six closure flags are false.

Ratio comparison uses the crate's decimal digit strings. The sources under review contain no `unsafe`. The overlay contains no `unsafe` and no `f64`.

## Preserved failure

`linux/proof` passed the same 23 tests, the 115-triple subset, and Clippy. Its overlay expected `reserved_class` for an approved row whose class `undetermined` is absent from the class map. The crate returned `core_enum`. That order matches the contract: `parse_enum_yaml` / `validate_enum` runs before `reserved_class`, and an approved class must exist on the enum. The oracle was wrong. The crate was not edited. `linux/proof/failed-a278_handcomputed.rs` is that overlay. `linux/proof/test.stdout` records 10 passed and `reserved_classes_are_not_business` failed, left `core_enum`, right `reserved_class`.

`linux/proof-2` expects `core_enum` for that document and expects `reserved_class` for a deprecated row that uses class `unexplained`. Those assertions passed with the other ten.

`linux/proof` offline fetch exited 101, `no matching package named serde_json`, on an empty private cargo volume. The online fetch in that container exited 0. `linux/proof-2` fetched offline from that same private volume and exited 0.

## Limits that stay in force

The observation vector is an operator fixture. This crate does not scan a database, write `oo_dict`, schedule a sweeper, certify a metric, or authenticate `owner` or `review.by`.

`historical_business_class` requires the current core `Review` fields named in the contract. Status text alone does not promote a row.

The image contains Python. The proof scripts did not invoke it. The registry check and the manifest tool are Rust.

## Not accepted here

SEM-D02, SP08 as a whole, a release, a database sweeper, hourly scheduling, an `oo_dict` write, metric certification, and customer owner proof are outside this child.
