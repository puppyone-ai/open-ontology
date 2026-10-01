# Dependency baseline (SP00-W01)

This file records what the workspace actually builds and which candidate crates were compiled outside it. The product is an agent-facing database and context entry. Nothing in this baseline implements that product. Structured fields are in [dependency-baseline.json](dependency-baseline.json).

Adoption clock: 2026-09-30T23:48:21Z. A crate published after 2026-09-23T23:48:21Z was not admitted. No security-fix exception was used. AGPL, BSL, ELv2, and SSPL are absent from the lock.

## Toolchain

`rust-toolchain.toml` pins channel 1.98.1 with rustfmt and clippy. The official channel manifest dated 2026-09-03 names rustc commit `48a229ceaefd4985c50990b14116b6d856af0985`. The compiler that ran the primary checks is Homebrew rustc 1.98.1 with that commit, host `aarch64-apple-darwin`. rustup is not installed, so Homebrew cargo does not switch on the toolchain file. The pin matches the compiler that ran.

Workspace `rust-version` is 1.94.0. That floor comes from the reviewed sqlx 0.9.0 candidate, which declares rust-version 1.94.0. sqlx is not in the lock. The highest `rust-version` among admitted crates is cargo-platform 1.91.

The same workspace was compiled and tested with official rustc 1.94.1, commit `e408947bfd200af42db322daf0fadfe7e26d3bd1`, installed under `/tmp` from the 2026-03-26 channel tarball. `cargo test --workspace --locked --offline` exited 0, including the 11 architecture tests. That install was not written into the user toolchain.

Linux `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu` rust-std artifacts are listed in the 1.98.1 channel manifest. They were not downloaded and nothing was linked for those targets.

## What the lock contains

Edition 2024, resolver 3, license Apache-2.0, `publish = false`. Sixteen members: the fifteen product crates plus `oo-arch-check`. Product crates have no features and no registry dependencies. `oo-core` has no dependencies at all.

The only direct external crate is `cargo_metadata` 0.23.1 on `oo-arch-check`, with default features off. The lock closure is 17 registry crates. Each checksum is the crates.io digest. License text was read from the crate's `LICENSE*` files, not only from the manifest field. Where a crate is MIT OR Apache-2.0, this project relies on Apache-2.0. `cargo_metadata` and `zmij` are MIT only. `memchr` is Unlicense OR MIT; this project relies on MIT. `unicode-ident` is `(MIT OR Apache-2.0) AND Unicode-3.0`; this project relies on Apache-2.0 and also on Unicode-3.0. Unicode-3.0 is not AGPL, BSL, ELv2, or SSPL.

| Crate | Version | Published | MSRV | License relied on |
|---|---|---|---|---|
| cargo_metadata | 0.23.1 | 2025-11-12 | 1.86.0 | MIT |
| camino | 1.2.6 | 2026-09-15 | 1.61.0 | Apache-2.0 |
| cargo-platform | 0.3.3 | 2026-04-18 | 1.91 | Apache-2.0 |
| itoa | 1.0.18 | 2026-03-20 | 1.68 | Apache-2.0 |
| memchr | 2.8.3 | 2026-07-08 | 1.61 | MIT |
| proc-macro2 | 1.0.107 | 2026-07-19 | 1.71 | Apache-2.0 |
| quote | 1.0.47 | 2026-07-19 | 1.71 | Apache-2.0 |
| semver | 1.0.28 | 2026-04-04 | 1.68 | Apache-2.0 |
| serde | 1.0.229 | 2026-07-18 | 1.56 | Apache-2.0 |
| serde_core | 1.0.229 | 2026-07-18 | 1.56 | Apache-2.0 |
| serde_derive | 1.0.229 | 2026-07-18 | 1.71 | Apache-2.0 |
| serde_json | 1.0.151 | 2026-07-20 | 1.71 | Apache-2.0 |
| syn | 3.0.6 | 2026-09-16 | 1.71 | Apache-2.0 |
| thiserror | 2.0.21 | 2026-09-23 | 1.77 | Apache-2.0 |
| thiserror-impl | 2.0.21 | 2026-09-23 | 1.77 | Apache-2.0 |
| unicode-ident | 1.0.26 | 2026-09-17 | 1.71 | Apache-2.0 AND Unicode-3.0 |
| zmij | 1.0.23 | 2026-07-13 | 1.71 | MIT |

serde is in the lock because `cargo_metadata` uses it. `oo-core` does not depend on serde. SP00-W02 owns the models.

## Reviewed candidates that are not in the workspace

These were compiled in `/tmp/oo-w01-probes` with `--locked`. `cargo check --locked --workspace --exclude probe-duckdb` exited 0. `cargo check --locked -p probe-duckdb` exited 0. A compile does not mean a protocol works.

Versions refused because they were younger than seven days, and the version that was compiled instead:

| Refused | Compiled |
|---|---|
| pg_query 6.2.1 (2026-09-30) | 6.2.0 |
| duckdb 1.10506.0 (2026-09-30) | 1.10505.0, feature `bundled` |
| encoding_rs 0.8.42 | 0.8.41 |
| jsonschema 0.58.0 through 0.58.3 | 0.57.0, default features off |
| hyper-util 0.1.21 | 0.1.20, pinned; reqwest 0.13.5 accepts it |
| tokio-rustls 0.26.6 | 0.26.5, pinned |
| rustls-platform-verifier 0.7.1 | 0.7.0, pinned |

Doc candidates that matched and compiled: tokio 1.53.1, axum 0.8.9, tower 0.5.3, tower-http 0.7.1, reqwest 0.13.5, sqlx 0.9.0, tokio-postgres 0.7.18, deadpool-postgres 0.14.2, arrow 60.0.0, parquet 60.0.0, calamine 0.36.1, object_store 0.14.2, serde-saphyr 1.3.0, utoipa 6.0.0.

Feature choices that stay in force when a later task admits these crates:

- tokio: `rt-multi-thread`, `macros`, `net`, `time`, `sync`, `io-util`, `signal`, `fs`. Not `full`. Its own default feature list is empty.
- reqwest: default features off, `rustls`, `http2`, `json`. Not `native-tls`, not `system-proxy`. The rustls feature uses aws-lc-rs. sqlx `tls-rustls` uses ring. Both providers in one future build is expected.
- sqlx: default features off, `runtime-tokio`, `tls-rustls` (ring-webpki), `postgres`, `mysql`, `sqlite-bundled`, `sqlite-deserialize`, `sqlite-unlock-notify`. The umbrella `sqlite` feature was not enabled, so `sqlite-load-extension` stayed off. `any` and `migrate` were not enabled. No database was contacted.
- serde-saphyr: `serialize` and `deserialize` only. `include` and `include_fs` stay off.
- jsonschema: default features off. The defaults fetch remote and file schemas.
- object_store: feature `fs` only.
- arrow and parquet stay on series 60 together. duckdb 1.10505.0 resolved arrow 58.4.0 in the same probe workspace. Do not force those series together.
- pg_query 6.2.0 ships libpg_query PG 17.7 (`PG_VERSION_NUM` 170007) and compiled against the host libclang. That compile is not evidence that the SQL gate accepts or rejects statements.
- tokio-postgres 0.7.18 source contains `query_portal` and `CancelToken::cancel_query`. Those paths were not executed.

The host resolve of the probe workspace (409 packages) had no crate younger than seven days, no prohibited license, and no `rust-version` above 1.94. The probe lock still contains wasm32 crates from reqwest 0.13.5 that were published on 2026-09-25 (`wasm-bindgen` 0.2.129, `js-sys` 0.3.106, `web-sys` 0.3.106, and the matching wasm-bindgen support crates). They are not in the Apple Silicon resolve. A wasm32 build of this combination is blocked.

rmcp 3.4.1, redis 1.7.0, and bollard 0.21.1 were read from crates.io and were not compiled. rmcp 3.5.0 and redis 1.7.1 were refused for age. MCP is SP13. The sandbox driver is SP10.

`cargo audit` and `cargo deny` are not installed. They were not run.

## Architecture gate

`oo-arch-check` reads `cargo metadata --locked` and fails closed. Unknown workspace members, missing required members, duplicate ids, a missing resolve graph, dangling resolve ids, unknown dependency kinds, and cycles are violations. Unknown kinds are still counted as edges.

The direct workspace edge set is exact. An extra edge and a missing required edge both fail. A new member, including the SP00-W05 `xtask` package, fails the gate until `REQUIRED_MEMBERS` and `REQUIRED_EDGES` in `crates/oo-arch-check/src/policy.rs` are updated.

Role rules enforced now:

- `oo-core` cannot reach another workspace crate, a client database driver, `bollard` / `kube` / `k8s-openapi`, or `async-openai` / `openai-api-rs` / `anthropic` / `rig-core`. serde is not in that deny list.
- `oo-gateway` cannot reach `oo-connectors`, `oo-pipeline`, or `oo-control`, including through other workspace crates. It cannot reach SQLx, mysql, duckdb, rusqlite, or the synchronous `postgres` crate. `tokio-postgres`, `postgres-types`, `postgres-protocol`, and `deadpool-postgres` are not in the gateway deny set. Sandbox and model crates on the gateway are `R-GATEWAY-RUNTIME`.
- `oo-sdk`, `oo-cli`, and `oo-mcp` cannot reach client database drivers, including the tokio-postgres stack and redis. A driver on `oo-sdk` fails `oo-cli` as well.
- `oo-analysis` cannot reach those drivers. The rust `duckdb` crate is in that set. A later sandbox DuckDB CLI is not a crate dependency.
- `oo-pipeline`, `oo-jobs`, `oo-control`, `oo-builder`, and `oo-connectors` are not under the transitive driver ban. A future sqlx dependency on the connector or job crate does not, by itself, fail the pipeline.
- `oo-eval`, `oo-entry`, `oo-sandbox`, and `oo-proxy` have no driver deny in this gate. The exact edge list still stops them from depending on connectors, pipeline, or control.

The negative check clones metadata, adds `oo-gateway` to `oo-pipeline` in memory, requires `R-GATEWAY-PIPELINE`, and checks the original metadata again. Manifests are not modified.

## Handoff

SP00-W02 owns `oo-core` models. This crate is a reservation, not those types.

SP00-W04 owns dataset generation. `oo-eval` is a skeleton. `bench/datagen` was not created.

SP00-W05 owns `xtask`. `.cargo/config.toml` already aliases `xtask` to `run --locked -p xtask --`. Until that package exists, `cargo xtask` fails because the package is missing (exit 101 in the W01 run). Register the new member in `oo-arch-check` in the same change that adds it.

SP00-W06 owns the license and the supply-chain policy. This baseline does not replace that policy.

`cargo run -p oo-cli --bin oo` prints that the command is not implemented and exits 2. The other process binaries do the same. A green workspace build is not a working gateway, pipeline, or control plane.
