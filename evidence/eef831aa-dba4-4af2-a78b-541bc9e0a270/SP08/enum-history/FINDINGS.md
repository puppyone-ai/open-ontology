# Findings

Reviewer: this session, sole coder and reviewer. This is not a separate person and it is not parent acceptance. The review build is `linux/review-1`, outside the development attempts. It uses a fresh target volume, the proof-3 sources, and the proof-1 child lock.

## Verdict

The frozen compiler matches `CONTRACT.md` on the runs below. `linux/review-1` is Linux arm64 Rust 1.94.0. Tests exited 0 (3 ratio, 2 CLI, 18 hand-computed). Clippy `--all-targets -D warnings` exited 0. `lock-subset` printed `registry=115 path=2 missing=0 mismatch=0`. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` was unchanged. Child lock `32ff28450510cd4cc375cc49ed52fe4be3a6a09af3d2a17b80c94c9ee90fa7a8` was unchanged. `cargo-update-invoked=no`. No PostgreSQL container was started.

I read `CONTRACT.md`, `src/lib.rs`, `src/main.rs`, `src/ratio.rs`, the fixtures, and the proof-3 and review-1 exits after those containers exited.

## What the runs show

`parse_enum_yaml` is the schema gate. The compiler then refuses reversed intervals, zero-length intervals, and overlaps, including a dormant future pair and a historical pair, before it classifies `as_of`. Adjacent bounds in the retroactive fixture stay two rows. A gap leaves both rows and binds `no_active`.

`undetermined` and `unexplained` are refused as class-map keys and as a class on any status other than the matching forced status. An approved inferred row needs `review.state=approved` and `review.by`. A rejected review on an approved row is `review_contradicts_status`. `review.at` stays optional. Dictionary and human approved rows compile without a review object. `owner_authenticated` stays false.

A superseded row without that review evidence stays `historical_unapproved` and contributes no business count. A deprecated row with the review evidence keeps status `deprecated`, sets `historical_business_class`, and counts only inside its interval. A superseded class that is absent from `classes` is `class_not_business`.

Unknown observed codes become warnings `unexplained` / `unknown_code` and are not inserted into the history. An active undetermined code warns with count `0` when it was not observed. Duplicate codes refuse. `u64` overflow refuses. Empty observations and an all-zero count leave coverage undefined: `defined=false`, ratio null, `review_needed=true`. The canonical fixture at `2026-10-06` reports observed `100/105`, declared coverage `0.97`, and does not write the YAML back. `95/100` meets `0.95`. `19/20` meets the spelling `0.950`. `94/100` is below.

The CLI reads one JSON object on stdin. A compiled empty observation exits 0. Overlap exits 2. A JSON number threshold exits 2 with `threshold_not_decimal_string`. Truncated JSON, extra arguments, and trailing bytes exit 1. Stdout is one JSON object. The six closure flags are false on the checked success and refusal documents.

Ratio comparison uses decimal digit strings. The hand-computed JSON rejects non-integer numbers. The sources contain no `f64` and no `unsafe`.

## Preserved failures

`linux/proof-1` passed the same 23 tests and the lock subset, then clippy rejected `Outcome` as `large_enum_variant`. The passing tree boxes `Report`.

`linux/proof-2` passed the tests on that boxed tree and exited 1 because `cargo clippy` was not installed in the new container. The component added during proof-1 lived in that container's root filesystem. proof-3 and review-1 install it again.

## Limits that stay in force

The observation vector is an operator fixture. This crate does not scan a database, write `oo_dict`, schedule a sweeper, certify a metric, or authenticate `owner` or `review.by`.

`historical_business_class` requires the current core `Review` fields named in the contract. Status text alone does not promote a row.

One active binding per code is the overlap refusal plus a last-match scan. The overlap fixtures cover a future pair, a historical pair, a gap, and an adjacent correction.

`count_overflow` also covers a fail-closed subtraction if a business sum ever exceeded the observed sum. The fixtures did not hit that branch.

## Not accepted here

SEM-D02, SP08 as a whole, a release, a database sweeper, hourly scheduling, an `oo_dict` write, metric certification, and customer owner proof are outside this child.
