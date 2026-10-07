# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a319`. Independent review of the frozen gateway HTTP dependency baseline.

Verdict: **APPROVE_LIMITED**.

The approval is the exact baseline only: unused workspace pins axum 0.8.9, tower 0.5.3, and tower-http 0.6.11, plus the five probe registry triples axum, axum-core 0.5.6, httpdate 1.0.3, matchit 0.8.4, and mime 0.3.17, with the archived loopback behavior. The decision file on disk stays `UNAPPROVED`. This review does not approve a consumer lock, a Gateway service, PostgreSQL cancellation, a parent goal, or a release.

## Archive and ownership

Fixed archive `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`, tree `539aa555a721a0b87b40546b9d4dad5569e3c863`. Proof built from `git archive` of that commit, mounted read-only at `/src`. Root `Cargo.toml` blob SHA-256 `b772f8fb4873885fcfac747eb24163559031bb9192ba99e8a8472571c75822bc`. Root `Cargo.lock` SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Decision blob SHA-256 `1307b6de43d8252acdcb0152f3745c8f1021a080a6a9772b56174e0666601181`.

Owned path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a319/SP00/gateway-dependencies-review/`. No root manifest, lock, toolchain, or product edit. `rust-toolchain.toml` remains channel `1.98.1`. The container set `RUSTUP_TOOLCHAIN=1.94.0`.

Author evidence under `eef831aa-dba4-4af2-a78b-541bc9e0a313` was read. The author's four same-session reviews are not this approval.

Recipe origins are in `recipes/ORIGIN.md`. `sha256_manifest.rs` and `lockdiff.rs` are unmodified copies. `crate_verify.rs` is the new oracle.

## Builder

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, rustc 1.94.0 (`4a4ef493e` 2026-03-02), host `aarch64-unknown-linux-gnu`. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `ALL`, no new privileges, network `oo-a319-eef831aa-net`, no Postgres. `/tmp` was `rw,nosuid,nodev,noexec,size=1g`. `TMPDIR=/target/tmp`.

Passing container `23920f1c098b793580f5ca3891dc15ac61463314b1be412e05ac0ee2567579b9`, name `oo-a319-preoverlay-2`, `inside_ok=2026-10-07T11:32:15Z`, `docker-run.exit` 0, OOMKilled false. Logs: `linux/preoverlay-2/`.

Earlier container `d713a1e5f7d388316fd850f13fc8abf9892420250213bfe9c81de3f9538128ad` exited 1. Its logs stay in `linux/preoverlay/`. See FINDINGS F4.

## Smoke, then the archived probe

`linux/preoverlay/check.stderr` is the cold scoped check: cargo downloaded the `cargo_metadata` 0.23.1 graph and checked `oo-arch-check` only, finished in 8.75s, exit 0. It did not fetch the workspace. `linux/preoverlay/fetch.stderr` is `cargo fetch --locked` of the probe manifest only.

`linux/preoverlay-2` reused the private cargo and target volumes. Its check finished in 0.04s and its fetch stderr is empty. It still re-ran metadata, the locked check, fetch, live crate verification, lockdiff, the four tests, and Clippy. Every phase exit in that directory is 0.

`root_pins=pass`. `members=pass` across 19 member manifests. `probe_manifest=pass`. Root manifest and both locks compared equal to the frozen blobs after the run (`lock=0`, lockdiff exact `added=0 removed=0 changed=0`).

## Crate bytes

Age gate is `7 * 86400` seconds before `2026-10-06T12:47:28Z`. Official `.crate` bytes, the cargo cache, and the lock checksum matched for each row. None are yanked. License files were read from the crate tarballs.

| Crate | Published | Age (seconds) | MSRV | License relied on | Checksum |
|---|---|---|---|---|---|
| axum 0.8.9 | 2026-04-14T07:55:20.258398Z | 15137528 | 1.80 | MIT | `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90` |
| axum-core 0.5.6 | 2025-12-27T09:19:34.014637Z | 24463674 | 1.78 | MIT | `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1` |
| tower 0.5.3 | 2026-01-12T18:43:05.227461Z | 23047463 | 1.64.0 | MIT | `ebe5ef63511595f1344e2d5cfa636d973292adc0eec1f0ad45fae9f0851ab1d4` |
| tower-http 0.6.11 | 2026-05-18T12:51:03.996149Z | 12182185 | 1.64 | MIT | `4cfcf7e2740e6fc6d4d688b4ef00650406bb94adf4731e43c096c3a19fe40840` |
| httpdate 1.0.3 | 2023-08-13T10:55:54.391658Z | 99366694 | 1.56 | Apache-2.0 | `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9` |
| matchit 0.8.4 | 2024-07-11T07:22:03.735375Z | 70608325 | undeclared | MIT AND BSD-3-Clause | `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3` |
| mime 0.3.17 | 2023-03-20T15:05:04.272177Z | 111966144 | undeclared | Apache-2.0 | `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a` |

Tower and tower-http are already in the root lock at those checksums (`root-lock-sha=pass`). The probe adds the other five. Lockdiff subset: `added=5`, `changed=0`, `denied=0`, `subset=pass`.

Workspace features: axum `http1` and `tokio`; tower `limit`, `make`, `timeout`, `util`; tower-http `limit` and `timeout`. Default features are off. The probe manifest has no JWT, OAuth, OIDC, or `auth` feature.

crates.io `max_stable_version` on 2026-10-07: axum 0.8.9, axum-core 0.5.6, tower 0.5.3, tower-http 0.7.1, httpdate 1.0.3, matchit 0.9.2, mime 0.3.17. See FINDINGS F3.

## HTTP proof

Archived `tests/loopback.rs`, `cargo test --locked --offline -- --test-threads=1`, exit 0. `test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.36s`.

- `echo_returns_body_and_ignores_auth_and_query` covers the HTTP/1 echo and the literal no-auth query-string echo.
- `body_at_limit_is_kept_and_one_extra_byte_is_413` covers 32 bytes against 33.
- `slow_route_returns_408_before_the_handler_finishes` covers the 408 deadline.
- `second_request_waits_for_the_single_permit` covers one shared `GlobalConcurrencyLimitLayer` permit across `/hold` and `/echo`.

`test_report=pass`. Clippy `--all-targets -- -D warnings` exit 0, finished in 7.81s.

## Limit of this verdict

`APPROVE_LIMITED` records that this baseline met the contract in `CONTRACT.md`. A later card still has to add the five triples to a consumer lock. That card is a separate approval. `execution/decisions/gateway-http-dependency-baseline.md` was not edited.
