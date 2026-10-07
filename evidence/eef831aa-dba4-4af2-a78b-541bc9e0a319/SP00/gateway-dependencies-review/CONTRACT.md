# Review contract

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a319`. Fixed archive `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`. This file is frozen before the proof run. It does not approve the baseline.

The review reads `execution/decisions/gateway-http-dependency-baseline.md` at that commit (blob SHA-256 `1307b6de43d8252acdcb0152f3745c8f1021a080a6a9772b56174e0666601181`) and the author `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`. The author's four same-session reviews are not an approval.

Owned path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a319/SP00/gateway-dependencies-review/`. No root manifest, root lock, crate manifest, toolchain, spec, or product edit.

## What this run proves

One private Linux arm64 builder, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, rustc 1.94.0, 6 GiB, 1 CPU, no Postgres. Smoke is `cargo metadata --locked --offline --no-deps` and `cargo check --locked -p oo-arch-check` on the frozen archive. Then the archived probe is fetched and its four loopback tests plus Clippy `-D warnings` run. Those four tests are the HTTP proof:

- `echo_returns_body_and_ignores_auth_and_query` is the HTTP/1 echo and the literal no-auth query-string echo.
- `body_at_limit_is_kept_and_one_extra_byte_is_413` is 32 bytes against 33.
- `slow_route_returns_408_before_the_handler_finishes` is the 408 deadline.
- `second_request_waits_for_the_single_permit` is the shared `GlobalConcurrencyLimitLayer` permit across the distinct `/hold` and `/echo` routes.

No extra test file is added unless one of those checks fails. A later expansion would be frozen before code.

Live crates.io metadata and `.crate` bytes are checked for axum 0.8.9, axum-core 0.5.6, tower 0.5.3, tower-http 0.6.11, httpdate 1.0.3, matchit 0.8.4, and mime 0.3.17. Age is at least `7 * 86400` seconds before `2026-10-06T12:47:28Z`. Declared MSRV must be at most 1.94.0. The httpdate edge is read from the axum, hyper-util, and hyper `Cargo.toml` bytes, including one-line feature arrays. The author sentence that hyper's `http1` feature pulls httpdate is not assumed.

The probe may add only axum 0.8.9, axum-core 0.5.6, httpdate 1.0.3, matchit 0.8.4, and mime 0.3.17. Shared registry triples must keep their version and checksum. Root `Cargo.lock` must stay byte-identical to SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

## Verdict rule

`APPROVE_LIMITED` applies only to this exact dependency baseline: the three unused workspace pins and the five probe triples, with the loopback behavior above. It does not approve a consumer manifest, a root lock edit, a Gateway service, PostgreSQL cancellation, or a release. A checksum, age, MSRV, license, triple-set, test, or Clippy failure is `WITHHELD`. A missing full HTTP consumer is not a baseline failure. No age waiver.

## Inventory

`SHA256SUMS` is produced after the proof host tail and the reports exist. The Rust hasher skips that output file and no other evidence file. The author's proof inventory gap stays a finding about the author tree. This review does not claim that older listing covers the host tail.
