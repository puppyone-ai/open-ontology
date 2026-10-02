# a073 SDK JSON query report

This is the executor record for the bounded `POST /v1/query` client in `oo-sdk`. It is not SP09-W01 acceptance, not AT-API-01, and not a PostgreSQL or Gateway end-to-end result. An independent review of the fixed commit still has to happen.

## Identity

Parent at the delivery commit: `d2ea6351414303ad1926fbeb99010607db8d2833`

The delivery commit hash and tree are recorded in `HANDOFF.md` after `git rev-parse`. This file is updated in the following owned commit so the hash is in the tree a reviewer checks out.

Workspace at authoring: `/Users/redux/puppy-one/open-ontology`

Owned paths:

- `crates/oo-sdk` (manifest, source, tests)
- `contracts/sdk/json-query` (contract and fixtures)
- `Cargo.lock` (root lock only; no other manifest)
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a073/SP09/sdk-json-query`

`oo-core`, xtask, pipeline, CI, and `ci/deny.toml` were not edited. No push, pull request, release, or customer write.

## What the client does

`oo-sdk` sends one HTTPS `POST` to a validated base plus `/v1/query`. The body is canonical JSON from `serde_json` (sorted object keys, null fields omitted). `parse_api_json::<QueryRequest>` must accept those bytes and round-trip to the same typed value before any socket opens. The response is read in chunks under the caller cap. `Content-Length` is not trusted and is not used as an allocation size. Success is HTTP 200 with `application/json` or `charset=utf-8` only. Other statuses must be a consistent `ErrorEnvelope` whose `ErrorCode::http_status` matches. Row count and row width are checked here; `oo-core` does not check those two.

Redirects are off (`Policy::none`). Proxies are off (`no_proxy`). reqwest's own retry is off (`retry::never`). TLS verification is never disabled. `RootTrust::OnlyPem` uses `Certificate::from_pem_bundle` and `tls_certs_only`, so hostname checks stay on. The bearer value is a `HeaderValue`. `Debug` and `Display` of the token and of `QueryError` do not include the token, SQL, URL, or body. `QueryError` has no `source()`. A timeout string says the client stopped waiting. It does not say the database cancelled work.

Automatic retry is only a complete `RATE_LIMITED` (429) or `OVERLOADED` (503) envelope with `retryable` true, status matched, at most 3 extra attempts, and a `Retry-After` that is one decimal-second value inside the configured maximum. A larger `Retry-After` is not shortened and is not retried. `INTERNAL` and `AUDIT_UNAVAILABLE` stay `retryable` on the returned error and are not replayed. Auth failures, `TIMEOUT`, transport failures, and protocol failures are not replayed. `RetryPolicy::none` disables the subset. This is not the full SP09 retry matrix.

`oo_sdk::UNSUPPORTED` lists export, arrow, discover, describe, resolve, search_values, freshness, from_sandbox_env, query_cancellation, and openapi_client.

The tests in `crates/oo-sdk/tests` use a synthetic loopback HTTP and TLS oracle. That oracle is not a Gateway and not PostgreSQL.

## Checks

### Pre-smoke, before this client

Run `574aa697-d37f-45b7-bd32-1cf19f0eadb1`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `RUSTUP_TOOLCHAIN=1.94.0`. Archive of `efb61d62803073b3f98f5fe1885877824f504f80` (that commit does not change `oo-sdk` or the root lock). Focused fetch, `cargo test -p oo-sdk`, and clippy. Phase exits and `docker-run.exit` are 0. Logs: `pre-smoke/`.

### Host, after the client

Toolchain: rustc 1.98.1 `(48a229cea 2026-09-01)`, host `aarch64-apple-darwin`, cargo 1.98.1. This is the default host toolchain, not the MSRV.

Preserved failures:

- `host/cargo-test-1.log` exit 101. Unit tests rejected `https://gateway.example/a/../b` too late because `Url` normalizes `..`, rejected `http://[::1]/` because `host_str` keeps brackets, and compared canonical JSON to insertion order instead of sorted keys.
- `host/cargo-test-2.log` exit 101. Gateway message still contained the loopback origin. `scrub_endpoint_text` keeps a proved host. The client now also strips its own origin from message and hint text.

Replacement, both exit 0:

- `cargo test --manifest-path Cargo.toml --locked --offline -p oo-sdk` (`host/cargo-test-3.log`). Lib 5, `http_query` 18, `proxy_env` 1, `tls_query` 1, doctests 0.
- `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-sdk --all-targets -- -D warnings` (`host/cargo-clippy-2.log`).

An earlier locked clippy in this session failed on `dead_code` in the shared test helper and on `clippy::manual_is_multiple_of` (`bytes.len() % 4`). That raw log was not archived. The helper allows `dead_code` because each integration test is its own crate. The length check is `bytes.len() & 3 != 0`, which 1.94 can compile. `cargo-clippy-2.log` is the green rerun. `cargo fmt -p oo-sdk` only. The workspace was not formatted.

### Linux arm64, Rust 1.94.0, after the client

Run `4ddb03eb-78b6-42ee-b66d-1cc7ea761226`. Same image digest. Container `oo-a073-msrv-4ddb03eb-78b6-42ee-b66d-1cc7ea761226`.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `cmake` is absent. `gcc` is Debian 14.2.0-19. `aws-lc-sys` 0.45.0 compiled anyway: the selected features are `prebuilt-nasm` only (not `ssl`, not `bindgen`), and that target has pregenerated bindings, so the crate's cc builder runs.

Snapshot: `git archive` of `9c7b98e4f7e92ab99a064ea4a839afe30319e4df` for `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, and `.cargo`, then overlay of the working `Cargo.lock`, `crates/oo-sdk`, and `contracts/sdk/json-query`. The contract directory is included because the tests `include_str` / `include_bytes` those fixtures. Later commits before the delivery parent change only `crates/xtask`. `oo-core` is unchanged from that archive to the delivery parent. The focused commands do not compile xtask.

Flags: `--pull never --init --user 0:0 --security-opt no-new-privileges --cap-drop ALL`, memory 8g, cpus 4, pids 4096, bridge network (not host), no socket mount, no HOME mount, `HOME=/tmp`, tmpfs `/tmp` `rw,nosuid,nodev,size=1g`, read-only source, task volumes for `CARGO_HOME` and the target dir. Inspect matches those settings. `Privileged=false`. `OOMKilled=false`.

Commands and exits (`linux-post/phases/*.exit`, `docker-run.exit`):

- `rustup component add clippy --toolchain 1.94.0`: 0
- `cargo fetch --manifest-path Cargo.toml --locked`: 0
- `cargo test --manifest-path Cargo.toml --locked --offline -p oo-sdk`: 0 (same counts as the host: 5, 18, 1, 1, 0)
- `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-sdk --all-targets -- -D warnings`: 0
- worst phase: 0
- docker run: 0

No full amd64 run. No workspace-wide test.

## Dependency provenance

Cutoff: 168 hours before `2026-10-02T01:08:59Z`, so `2026-09-25T01:08:59Z`. crates.io user-agent `open-ontology-a073-audit`. Compared with `HEAD:Cargo.lock` at audit time. Result: 131 added registry versions, 0 removed, 0 yanked, 0 checksum mismatches, 0 younger than the cutoff, 0 fetch errors. Per-crate records: `provenance/crates.json`. `cargo audit` and `cargo deny` were not run. This is not supply-chain admission. `ci/deny.toml` was not edited.

The lock update was incremental. An unlocked `cargo check -p oo-sdk` created the first lock and exited 101 on later compile errors (`QueryId` privacy, `Endpoint` debug). Those errors were fixed in `oo-sdk` only. Five versions inside the 168-hour window were then moved with `cargo update -p <crate> --precise <older>`: `cc` 1.5.1 to 1.4.7, `find-msvc-tools` 0.1.14 to 0.1.13, `yoke-derive` 0.8.4 to 0.8.2, `quinn-proto` 0.11.19 to 0.11.18, `quinn-udp` 0.5.16 to 0.5.15. `cargo generate-lockfile` was not used. No previously published sole version was replaced. Extra versions sit beside existing ones for `base64` 0.22.1, `cpufeatures` 0.3.1, `getrandom` 0.2.17 and 0.4.3, and `r-efi` 6.0.0. `serde` 1.0.229 and `serde_json` 1.0.151 were already in the root lock.

Direct pins (exact, `default-features = false` where the manifest says so):

| Crate | Version | Created | MSRV field | License | Checksum |
| --- | --- | --- | --- | --- | --- |
| reqwest | 0.13.5 | 2026-09-08 | 1.85.0 | MIT OR Apache-2.0 | `16a1cfa75cc186dd73d5818e510e042e40927bccc9c236b061cea97e1eb08029` |
| tokio | 1.53.1 | 2026-07-20 | 1.71 | MIT | `202caea871b69668250d242070849eb495be178ed697a3e98aebce5bc81a0bed` |
| hyper-util | 0.1.20 | 2026-02-02 | 1.64 | MIT | `96547c2556ec9d12fb1578c4eaf448b04993e7fb79cbaad930a656880a6bdfa0` |
| tokio-rustls | 0.26.5 | 2026-09-04 | 1.71 | MIT OR Apache-2.0 | `b0c85f2c3ef0b1cd58b36682f4b17aaa995f0e5db534d85692b4903abce21f67` |
| rustls-platform-verifier | 0.7.0 | 2026-04-12 | 1.85 | MIT OR Apache-2.0 | `26d1e2536ce4f35f4846aa13bff16bd0ff40157cdb14cc056c7b14ba41233ba0` |
| cc | 1.4.7 | 2026-09-18 | 1.65.0 | MIT OR Apache-2.0 | `54413ede23c2daf518f35156dfde027feb2374004d63bd497f983c8db9c0e313` |
| find-msvc-tools | 0.1.13 | 2026-09-18 | 1.65.0 | MIT OR Apache-2.0 | `ef25905e51abafe4dcea6c15fec58c57b601cdbd0ee53d22ea1d3016c587d39b` |
| yoke-derive | 0.8.2 | 2026-04-01 | none | Unicode-3.0 | `de844c262c8848816172cef550288e7dc6c7b7814b4ee56b3e1553f275f1858e` |
| rustls (dev) | 0.23.45 | 2026-09-14 | 1.71 | Apache-2.0 OR ISC OR MIT | `0d41d731c7d2f962d1ccc364cec258de3c0e93b38c2fb3ba97ac74513048d634` |
| socket2 (dev) | 0.6.5 | 2026-07-13 | 1.70 | MIT OR Apache-2.0 | `c3d1e2c7f27f8d4cb10542a02c49005dbd6e93095799d6f3be745fae9f8fedd4` |

reqwest features: `rustls` and `http2` only. That selects `__rustls-aws-lc-rs`, rustls-platform-verifier, and `__rustls`. It does not select `json`, `stream`, `native-tls`, or the system proxy. Tokio library features: `time` only. Tests add `macros`, `rt-multi-thread`, `net`, `io-util`, and `sync`. The `process` feature is off. Dev rustls features: `std`, `tls12`, `aws-lc-rs`. Dev tokio-rustls features: `tls12`, `aws-lc-rs`.

`cargo tree --locked --offline -p oo-sdk` (normal, `--target all`, and `--target aarch64-unknown-linux-gnu`) does not show `quinn`, `ring`, or `native-tls`. Those crates are still in the lock as optional reqwest edges. `quinn` 0.11.12, `quinn-proto` 0.11.18, and `quinn-udp` 0.5.15 are locked and unselected by those three queries. `ring` 0.17.14 is locked and unselected. `webpki-root-certs` 1.0.9 appears once under `--target all` and not on the host or linux/arm64 queries. Its license is `CDLA-Permissive-2.0` only, which is outside the `ci/deny.toml` allow list. No exception was added.

`openssl-probe` 0.2.1 and `rustls-native-certs` 0.8.4 are on the linux/arm64 tree through rustls-platform-verifier. They read the platform certificate store. They are not an OpenSSL TLS backend and not native-tls. `dunce` 1.0.5 (`CC0-1.0 OR MIT-0 OR Apache-2.0`) is selected via aws-lc-sys. `r-efi` 6.0.0 is `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. Thirty added crates publish no `rust_version`. None of the published `rust_version` fields are above 1.94. The host graph and the linux/arm64 graph compiled. That does not prove the MSRV of every unselected locked crate.

`zeroize` is present transitively. This SDK does not call it. Token memory is not wiped on drop. `aws-lc-sys` 0.45.0's license expression is a conjunction of ISC, Apache-2.0, MIT, BSD-3-Clause, and MIT-0 terms. It was not passed through cargo-deny.

wasm32 was not built. wasm-bindgen 0.2.129 was not added. Do not treat wasm as admitted.

## Cleanup

`cleanup.txt` at `2026-10-02T01:13:02Z`. Removed only a073 containers, bridge networks, and volumes for runs `574aa697-d37f-45b7-bd32-1cf19f0eadb1` and `4ddb03eb-78b6-42ee-b66d-1cc7ea761226`, plus the two `/tmp/oo-a073-<run>` snapshot directories. Each `docker rm`, `docker network rm`, and `docker volume rm` exited 0. No prune. No foreign container was removed. No `oo.kind=rust-builder` container was running afterward.

## Gaps that stay unsigned

Full SP09-W01, AT-API-01, OpenAPI publication, query cancellation, CLI conversation, real PostgreSQL end-to-end, dual-arch matrix, cargo-audit, cargo-deny, and supply-chain admission are not done. The synthetic oracle is not those proofs.
