# Gateway HTTP dependency contract

Status: plan, written before any root manifest edit.
UUID: `eef831aa-dba4-4af2-a78b-541bc9e0a313`
Fixed archive: `b91b67bee0f603725b2cb7cc9c284588a2699392`
Saved at: `2026-10-06T12:47:28Z`
This baseline stays **UNAPPROVED** until an independent review. It is not a Gateway service, not a public API, and not a new product unit.

## Authority

- `docs/02-technology-stack.md` §1, §5, and §6. Tokio / Axum / SQLx candidate series is 1.x / 0.8.x / 0.9.x. A release younger than 7 days does not enter the default artifact. Cargo default features are not a security configuration.
- `docs/07-gateway.md` §3. Gateway tokens are opaque strings. The Gateway stores a hash. This change does not add JWT, OIDC, OAuth, or query-claim libraries.
- `docs/17-rust-implementation.md` §2 and §7. HTTP routing is Axum / Tower. Body size, concurrency, and request deadline are bounded. A Tower timeout does not cancel PostgreSQL work. This card does not implement that cancellation.
- `supply-chain/policy.md` §1 and §4. Exact version, official source, RFC3339 publication time, MSRV, features, license evidence, and checksum. `audited` stays false. No advisory scanner is claimed.
- `supply-chain/dependency-baseline.md` already recorded a compile outside the workspace for axum 0.8.9, tower 0.5.3, and tower-http 0.6.11 as the reqwest line. That file is not edited here.
- Root writer history on a207 stays in place. This card is the coordinator transfer for this gateway dependency change only. Connector manifests stay a207-owned and are not edited.

## What is already frozen

Root `Cargo.lock` at the fixed commit and at the working HEAD are the same git blob (`2990f901ea98a7b465f462cce8742bc5c51c13aa`). `Cargo.toml` is blob `c78ee5d0575a11cd6078cf0055cdd2c673edf583`. There is no `axum` package. `tower` 0.5.3 checksum `ebe5ef63511595f1344e2d5cfa636d973292adc0eec1f0ad45fae9f0851ab1d4` and `tower-http` 0.6.11 checksum `4cfcf7e2740e6fc6d4d688b4ef00650406bb94adf4731e43c096c3a19fe40840` are already in the lock. `oo-arch-check` rejects an unknown workspace member (`R-MEMBER-UNKNOWN`) and its tests call `check(metadata()).is_pass()` on the live workspace. `crates/oo-arch-check/src/policy.rs` is not an owned path, so this card does not add a workspace member.

## Official versions selected

Retrieved from `https://crates.io/api/v1/crates/...` and the crate files at `https://static.crates.io/crates/...`. The `.crate` SHA-256 values below match the crates.io `checksum` field. Local tarball check on 2026-10-06: axum, tower, and tower-http matched. Axum's crate `updated_at` is `2026-04-14T07:55:20.258398Z`, which is also the publish time of 0.8.9. No newer Axum release is published. 0.8.9 is the 0.8 line head, not a version below that line, and it is older than 7 days. Tower 0.5.3 is the newest Tower 0.5 release and the version already locked. tower-http 0.7.1 exists and is older than 7 days, but Axum 0.8.9 depends on tower-http `^0.6.8`. The root lock already has 0.6.11. This card reuses 0.6.11 and does not add the 0.7 line.

| Crate | Version | Published (UTC) | MSRV | License field | Checksum |
|---|---|---|---|---|---|
| axum | 0.8.9 | 2026-04-14T07:55:20.258398Z | 1.80 | MIT | `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90` |
| tower | 0.5.3 | 2026-01-12T18:43:05.227461Z | 1.64.0 | MIT | `ebe5ef63511595f1344e2d5cfa636d973292adc0eec1f0ad45fae9f0851ab1d4` |
| tower-http | 0.6.11 | 2026-05-18T12:51:03.996149Z | 1.64 | MIT | `4cfcf7e2740e6fc6d4d688b4ef00650406bb94adf4731e43c096c3a19fe40840` |
| axum-core | 0.5.6 | 2025-12-27T09:19:34.014637Z | 1.78 | MIT | `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1` |
| matchit | 0.8.4 | 2024-07-11T07:22:03.735375Z | not declared | MIT AND BSD-3-Clause | `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3` |
| mime | 0.3.17 | 2023-03-20T15:05:04.272177Z | not declared | MIT OR Apache-2.0 | `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a` |

Axum's normal dependency on matchit is `=0.8.4`. Axum's normal dependency on axum-core is `^0.5.5`, and 0.5.6 is the highest published 0.5 release. mime `^0.3.16` resolves to 0.3.17, which is also what tower-http's optional mime edge asks for (`^0.3.17`). All three are older than 7 days as of `2026-10-06T12:47:28Z`.

License reliance, after the crate `LICENSE` bytes are extracted in the proof run:

- axum, tower, tower-http, axum-core: MIT, the only license in the crates.io field.
- matchit: MIT AND BSD-3-Clause. Both obligations stay. BSD-3-Clause is permissive. It is not AGPL, BSL, ELv2, or SSPL.
- mime: MIT OR Apache-2.0. This project relies on Apache-2.0, the same choice as the existing baseline for dual-licensed crates.

`TimeoutLayer::new` in tower-http 0.6.11 is deprecated since 0.6.7. The probe calls `TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, ...)`.

## Features

Root `[workspace.dependencies]` pins, default features off:

- `axum` `=0.8.9`, features `http1` and `tokio` only. That is the documented minimum for `axum::serve` on HTTP/1. `query`, `form`, `json`, `ws`, `macros`, `tower-log`, `tracing`, `matched-path`, `original-uri`, and `http2` stay off. The `query` feature is the query-string extractor, not a claims system. It is still left off.
- `tower` `=0.5.3`, features `limit`, `make`, `timeout`, and `util`. Axum already asks for `util` and, through its `tokio` feature, `make`. `limit` is the concurrency bound. `timeout` is Tower's own deadline middleware. Tower 0.5.3 publishes no default-feature list. `full` stays off. `log` stays off.
- `tower-http` `=0.6.11`, features `limit` and `timeout` only. `limit` is `RequestBodyLimitLayer` (413 when `Content-Length` exceeds the cap). `timeout` is the HTTP deadline layer (408). Its `auth` feature stays off. tower-http 0.6.11's default feature list is empty.

No consumer crate manifest changes. `oo-gateway` does not gain an Axum dependency in this card. Later consumer work has to use a fixed archive after review and opt in with `workspace = true`.

`oo-arch-check` cannot see an unused workspace dependency as a member. The root lock therefore stays byte-identical unless `cargo metadata --locked` on the edited manifest proves Cargo would rewrite it. A rewrite that changes any existing registry name, version, or checksum is a stop. Additions are allowed only when a workspace member actually depends on the new crates. This card does not add that member.

## Probe and lock recipe

The probe is a separate workspace at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a313/SP00/gateway-dependencies/probe`. Its own `[workspace]` table keeps it out of the product workspace. It repeats the three workspace pins and exact-pins every overlapping crate below so the resolver cannot float them:

`axum-core` 0.5.6, `bitflags` 2.13.2, `bytes` 1.12.1, `futures-core` 0.3.34, `futures-util` 0.3.34, `http` 1.5.0, `http-body` 1.1.0, `http-body-util` 0.1.5, `hyper` 1.11.1, `hyper-util` 0.1.20, `itoa` 1.0.18, `matchit` 0.8.4, `memchr` 2.8.3, `mime` 0.3.17, `percent-encoding` 2.3.2, `pin-project-lite` 0.2.17, `serde_core` 1.0.229, `sync_wrapper` 1.0.2, `tokio` 1.53.1, `tokio-util` 0.7.19, `tower-layer` 0.3.3, `tower-service` 0.3.3, `tracing` 0.1.44.

Those overlap versions are copied from the frozen root lock. The recipe is:

1. `cargo fetch --manifest-path probe/Cargo.toml` with a private cargo home. Not `cargo update` of the root workspace, and not an unpinned `cargo add`.
2. Rust `lockdiff --subset` compares the probe lock with the frozen root lock. Every registry package whose name is already in the root lock must keep that version and checksum. Probe-only packages must be `axum`, `axum-core`, `matchit`, and `mime`, with the checksums in this contract, and an age of at least `7 * 86400` seconds at `2026-10-06T12:47:28Z`.
3. Names containing `jwt`, `jose`, `oidc`, `oauth`, `openid`, `aliri`, or `paseto` fail the gate.
4. If a same-name checksum or version differs, try one `cargo update -p NAME --precise ROOT_VERSION` for that package only. If the root version cannot satisfy the requirement, stop and write the conflict. Do not edit the root lock to follow a newer version in this card.
5. The probe then `cargo test --locked --offline` and `cargo clippy --locked --offline --all-targets -- -D warnings`.

The probe binds `127.0.0.1:0` and speaks HTTP/1.1. It checks a 32-byte body cap (413 above it), a concurrency permit of 1, and a 150ms deadline on `/slow` (408). A successful `/echo` returns the raw body and does not read an `Authorization` header or a query string. There is no login, token issue, or query API.

## Pre-edit smoke

Before the root `Cargo.toml` edit, one private Linux arm64 builder runs `cargo fetch --locked -p oo-arch-check` and `cargo check --locked --offline -p oo-arch-check` on a `git archive` of `b91b67bee0f603725b2cb7cc9c284588a2699392`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel 1.98.1. That toolchain file is not edited. Memory 6 GiB, 1 CPU, cap-drop all, no privileged mode, no host network, no host home, no Postgres. If this smoke fails, the manifest is not edited.

## Out of scope

Consumer manifests, child locks under `tools/`, `benchmarks/`, and evidence snapshots, `rust-toolchain.toml`, docs, specs, products, and `supply-chain/dependency-baseline.json`. No public Gateway readiness. No whole-workspace or dual-architecture build.
