# SP08-W01A business calendar

## Scope and verdict

This child adds a standalone Rust workspace at `tools/business-calendar`. It resolves one validated OOS calendar YAML at one explicit RFC3339 `as_of` instant. Configured anchors and `[start, end)` phrases become civil dates, with business-zone midnight boundaries when that local midnight exists once.

The caller supplies `last_closed_period_end`, a closure label, and a declared record hash when that anchor is configured. `finance_approval` and `authenticated` stay false. The record hash is provenance supplied by the caller. The helper does not recompute it and does not approve a financial close.

This report does not claim parent acceptance, SEM-D01 acceptance, a database calendar, an authenticated finance audit, or a release.

Fixed commit `0bcd494457091d4b55c82fc6b9922b5bc5dc44c4`, tree `fafa03de58bee2f37df65f9d3691d1aa99290fc5`. The product proof archives that commit and overlays only `tools/business-calendar`. The root `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. a207 remains the connector manifest and root lock writer. No root manifest or root lock was edited. `cargo update` was not invoked.

Contract: `tools/business-calendar/CONTRACT.md`, written before the Rust sources.

## Recipes

Launch and report shape follow reviewed recipes. The UUID, fixed commit, owned paths, 6 GiB limit, and 1 CPU limit are this child's values. PostgreSQL was not started.

| Origin | Commit | Path | SHA-256 |
|---|---|---|---|
| a204 preoverlay launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 preoverlay inside | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a199 manifest tool | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| a270 review launch | `8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a270/SP08/enum-history/linux/review-1/launch.sh` | `21befcad6c519b7edffdc75431a1d2aa62860632ab10d8cd4f2a0d5d4b7c477d` |
| a270 review inside | `8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a270/SP08/enum-history/linux/review-1/inside.sh` | `5b3849341ac30156b344e8bebf90963714d07ef4ba6acf74beb207c4124eefc4` |

`tools/sha256_manifest.rs` matches the a199 blob. `tools/lock_subset.rs` is the Safe Rust registry checker for this child. Path packages must be exactly `business-calendar` and `oo-core`. Overlay copies use `cp -R`.

## Pre-edit smoke

`linux/precode` on the pinned Linux arm64 Rust 1.94 image, 6 GiB, 1 CPU, network none, dropped capabilities, no pull. The fixed archive did not contain `tools/business-calendar`. Probe stdout: `msrv_smoke_ok calendar_helper=absent postgres=0 clock=explicit`.

`docker-run.exit` 0, `probe-run.exit` 0, OOMKilled false, privileged false. Container id `56c6282468b1310e73d692c5db0ebabfbfee543dc844f7f5646c263fc6ee8b8c`.

## Decisive verification

`linux/impl-4` is the passing product run. It copied the fixed archive, moved `rust-toolchain.toml` aside, overlaid the tool, fetched from a copy of the root lock, and checked the subset. `lock-subset` reported `registry=115 path=2 missing=0 mismatch=0`. A later fetch used `--locked --offline`. The root lock hash after the run was still `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. The generated child lock is `7a9470817fc50ce83b44faacd4cd6330b3ee6d76d22a0e0bf9cbb75532f83cf5` and is copied to `tools/business-calendar/Cargo.lock`.

Tests, from `linux/impl-4/test.log`:

- library: 8 passed
- binary unit: 0 passed
- `tests/cli.rs`: 5 passed
- `tests/handcomputed.rs`: 8 passed
- doctests: 0 passed

`cargo clippy --locked --offline --all-targets -- -D warnings` exited 0. `smoke.exit` 0, `docker-run.exit` 0, OOMKilled false, privileged false. `cargo-update-invoked.txt` is `no`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL. Container id `56f0fa1001fffb50f81405580f1095520af7568036d9d90bbf4572a7a336ebed`.

`linux/review-1` repeated the same tests and clippy on a fresh target volume, reusing the private cargo cache and the generated lock. It did not regenerate the lock. `review=ok`, `smoke.exit` 0, `docker-run.exit` 0. The same test counts passed. Root and child lock hashes were unchanged on the host before and after. Container id `33377f9bc77755fc0649171c9efdcb26647807d640c9e21f2dec9f72a9d3eff6`.

`rustc` reported `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

## Retained failures

`linux/impl-1`, `linux/impl-2`, and `linux/impl-3` are preserved.

- impl-1 exited 101. The library imported `refused` twice and called `year`, `month`, and `day` without `Datelike`. Container `7a8374e368ec89e1c37ca2df516b8b583bbd1d8dbd6068a0e0aebd9dc33cdf0d`. OOMKilled false.
- impl-2 exited 101. Library tests passed. `bounds_and_redaction` compared serde_json map iteration, which sorts keys, with struct field order. Clippy rejected a manual `Default` and an `i32::try_from` of an `i32`. Container `a0f09d1cdeba02cd19a940f13fd086dc45c06d684e364427f7fd53a1884c9fc7`.
- impl-3 exited 101. Clippy exited 0. Twenty-one focused tests passed. `source_does_not_read_a_clock` matched `std::process::ExitCode`. Container `b9fbe234a77133a024a959d36eca5fd1dc40ddae8a6ebcf697225795471e0c99`.

## What the helper does

`business_tz` is an IANA name through chrono-tz 0.10.4 and chrono 0.4.45. `quarter_convention` is `business_fiscal_quarter`: three civil months from `fiscal_year_start_month`. `week_start` is metadata. There is no `week` anchor. Phrase labels are emitted in `BTreeMap` order. `inclusive_end_civil` is the civil day before the exclusive end. A missing or ambiguous local midnight blocks the phrase or refuses the anchor. It is not shifted onto a nearby instant.

The stdin CLI reads one `oo.business-calendar/v1` JSON document. Exit 0 writes the resolved JSON and a newline. Refusals use the exit table in `CONTRACT.md` and the stderr line `business-calendar: refused <reason>`.

## Product hashes

| File | SHA-256 |
|---|---|
| `.gitignore` | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` |
| `CONTRACT.md` | `6c20caef62fc46216d116e550858bbd98471ffb4b3fe7056f5a7234565d61095` |
| `Cargo.lock` | `7a9470817fc50ce83b44faacd4cd6330b3ee6d76d22a0e0bf9cbb75532f83cf5` |
| `Cargo.toml` | `8a4a6f5cfd7705f74a4d543e295c4d799be5faeabf7d269832e7b410c7923190` |
| `src/civil.rs` | `d4817ecf33344df4051f7e0edae119a46a77b437fa6b25dc2ff41dd7f13f84ca` |
| `src/expr.rs` | `a242900ce28f9ef3b3b6e7e7cdf2bfc344f8de8d389cdaddb35b4ad65f6a1d7f` |
| `src/fault.rs` | `9838917bbbd702e9f2f0830d26ad37221e543f47216ab5c980b94f07e5834b9f` |
| `src/lib.rs` | `39d5dd6d5f88d9077d2b014067101f41e59e23351d78c8579c8b345dddfef2b3` |
| `src/main.rs` | `586f8cc619e4ad75887ffe4302ec2de72dd717cd11f86447d0e63df17d770255` |
| `src/sha256.rs` | `898c967c4e40a929f0b57b4d50d6b7759edff5aeb90418644770d8d2aa0d2898` |
| `src/zone.rs` | `3c9ed877788a8c3b91bc0422d863b0f2ed3c75b3dde286192ecc76f43f308693` |
| `tests/cli.rs` | `d4d6ebfb3534f0fc55e01bc9d4f2a1b6722c49a3f8e40c7e50668f3e3172098d` |
| `tests/handcomputed.rs` | `eb835e6d8c2017e45ffd907ed5f8afbd0d276506138bb240f8f8a71f4d029179` |

Copies of those bytes are under `source/blobs/tools/business-calendar/`. The full evidence inventory is `SHA256SUMS`.
