# a083 PostgreSQL source integration

Local integration of the accepted PostgreSQL source correction onto current main. This is a development delivery. It is not release approval, runsc approval, protected-CI approval, customer approval, PostgreSQL 13/14 coverage, or full SP03 admission. An independent fixed-SHA review follows. Root Cargo ownership is released with the commit that contains this report. The NFC dependency task is not started here.

## Identities

| Item | SHA |
|---|---|
| Frozen baseline (Linux 1.94 ran here) | `78d106fcb3e7e1705b617ed6df58933d85297e7b` |
| Baseline tree | `66010b8c215df498afbc585b5461b6668ae58cd9` |
| Baseline parent | `a45c13c0e231dcbf6c7b97050b9bfbe8a11bb571` |
| Integration commit | `2980e85af22c3ade05272c66c7d2835b52730322` |
| Integration tree | `c027c4599cdebf701b21e97e51ded45b9067ca8e` |
| Commit parent at delivery | `1868ffbca151c64a21efd6d1a09cb4806b1e95f3` |
| Commit-parent tree | `66a4e486b0e552766218f35eeee3fa69a271d928` |
| SDK delivery | `8efeb230afc6734a2e50eaca3f09fd78300a9c7d` |
| SDK stamp | `0b544cb066292221016899c9b5eceef84e984227` |
| Accepted correction | `4ecdbf714766baef3490a4f954701eaf19f245a0` |
| Correction parent | `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` |
| Original product source base | `4213134d136ff50bd2f7d422f3f840e48e038b6d` |
| SQLx upstream VCS for license bodies | `003b698e99e024f3621b8043a2426fde5b741171` |
| Product-pg checkout, left in place | `181df9b6ba030b68e3a48aa0f58e5c5f909bee55` |
| Merged `Cargo.lock` | `9b8f7e50d3b20042bb2d606f47276b76dd68f3568136343040fbea1ecd622057` |

`78d106fc..1868ffbca` does not change `Cargo.toml`, `Cargo.lock`, `crates/oo-connectors`, `crates/oo-sdk`, `crates/oo-cli`, `crates/oo-gateway`, or `ci/deny.toml`. The measured graph and the SDK test apply to that parent. The integration commit is `2980e85af22c3ade05272c66c7d2835b52730322`.

## What was integrated

Source came from `git archive` of `4ecdbf71`. The product-pg checkout was not moved. No branch merge. `xtask`, `oo-core`, infra, jobs, `oo-builder`, `oo-pipeline`, and the standalone spike were not edited.

Byte compare of `crates/oo-connectors`, `contracts/source/pgsql`, and `testing/product/pg-source` against that archive: 244 files identical, 0 files added, 0 files removed, 6 files different. The differences are the full license bodies, the compile-time guard in `vendor/sqlx-core/src/lib.rs`, and the identity text in `vendor/PATCH.md`. Recorded in `provenance/byte-identity.json`.

Root `Cargo.toml` gains only the path patch:

```toml
[patch.crates-io]
sqlx-core = { path = "crates/oo-connectors/vendor/sqlx-core" }
sqlx-postgres = { path = "crates/oo-connectors/vendor/sqlx-postgres" }
```

`ci/deny.toml` was not edited. ISC stays allowed because that correction is already on main. The allowlist was not widened for CDLA or BSL.

The connector keeps explicit config, `VerifyFull` with the caller CA only, and fail-closed admission for reachable write privileges. Frozen protocol values are `oo.source.pgsql/v1`, `pgident/v1`, empty schema hash `cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc`, and reader schema hash `b0dbcffe78f8818af5c72f37c2db048c7058ccfaf8eb1eef4b17b83ac37d9692`. The comment canary SHA is `99a042fad752401594c711cdc674f22470412394789d6e485622b0de7f264d50`. The live test asserts the empty and reader schema hashes. `empty-catalog.json` file sha256 is `044022fcabb7587632133cb8f81c3665292c32acc7af3432b5bfe4d066679a3c`; the protocol hash above is the `schema_hash` field inside that file.

## Vendor identity

These are modified upstream artifacts. `sqlx-core` and `sqlx-postgres` are path patches and have no crates.io checksum in the lock. The registry `sqlx` 0.9.0 checksum matches the downloaded crate.

| Artifact | sha256 |
|---|---|
| `sqlx-0.9.0.crate` | `378620ccc25c62c89d8be1c819e76a88d59bdcc3304733330788948e619bfd71` |
| `sqlx-core-0.9.0.crate` | `05b44e85bf579a8eeb4ceaa77a3a523baf2bf0e9bac7e40f405d537b5d2d5ccb` |
| `sqlx-postgres-0.9.0.crate` | `87a2bdd6e83f6b3ea525ca9fee568030508b58355a43d0b2c1674d5f79dcd65e` |
| VCS `LICENSE-MIT` (1109 bytes), both crates | `5abbdd842310c8f1650fcde3a5a51a3e09412b9e5a6eea87519e0db9e32b339d` |
| VCS `LICENSE-APACHE` (10297 bytes), both crates | `c8f5453612253e8ea8bad241617fe74c4a6d5d592ba64ff881668c211b50ac99` |
| a074 original unified patch | `f4b1aff1f2575a6b161a5cec37da26a4e097475aa1dcde3a9cec603766ac5f37` (3664 bytes, 104 lines) |
| Path-stable `git diff --no-index` before integration | `0129343a96be05b31dabc2824be8b2d587ac43d4eddde656c4dcad17f6c4948e` (4311 bytes, 112 lines) |
| Same command after integration | `20127ad930a8a96dbdbeac15309cb1404a5c58f15fedf70afe55726fa6562480` (29830 bytes, 623 lines) |
| Integration versus the original vendor tree | `b5190be5f2dacc306982f47b278a7b652f9761702c22127546808cfa4c272e1d` (29831 bytes, 568 lines) |

The a074 patch hash and the path-stable diff are different serializations of the same four-file change. They are not claimed to be byte-identical.

Tree sha256 of file lists, from `provenance/patch-hashes.json`:

| Tree | sqlx-core | sqlx-postgres |
|---|---|---|
| Official tarball | `c130816abec44203e7e2816f72fde858e84d97fe69a74c15a15d5f2697dc25c8` | `b6f4bccefb5fb0091bb91e315e2586e2bb06d01466178a023f73a5f601fba6d4` |
| Original patch | `365f8f4b4554a61585c6e781917850bb4627ad751267d503fb6ddccf2ee0451d` | `68419d6649a9b19a003d0eacead5d4e882125ccbec57db15fdcfafd461f5af7e` |
| Integrated | `aa8e4d6bfdd56979fcd5c024d40d2205cdea241c417232b2a740c0d30641df63` | `6731a283c6e4d58602543579f35e0d568c8ee03e6240bdedd96cd275cc2307c4` |

Published crate `LICENSE-MIT` and `LICENSE-APACHE` are 14 and 17 byte pointer files. The vendored copies are the full VCS texts. No license-policy exemption was added.

Original patch files versus the tarballs: `sqlx-core` `Cargo.toml`, `Cargo.toml.orig`, `src/net/tls/tls_rustls.rs`; `sqlx-postgres` `src/options/mod.rs`. Integration adds the two license bodies and `sqlx-core/src/lib.rs` on top of that. File counts stay 110 and 119. No extra or missing files.

## Lock

`cargo fetch` on Linux 1.94 updated the existing lock. `cargo generate-lockfile` was not used. 267 packages became 305. 38 added, 0 removed, 0 same-version source or checksum changes. Direct pins kept: `serde` 1.0.229, `serde_json` 1.0.151, `sha2` 0.10.9, `tokio` 1.53.1, `zeroize` 1.9.0, `ring` 0.17.14. A second `sha2` 0.11.0 was added beside 0.10.9. The same pattern holds for `block-buffer` 0.12.1, `crypto-common` 0.2.2, `digest` 0.11.3, and `hashbrown` 0.16.1.

Age audit in `provenance/added-crates.json`, cutoff `2026-09-25T00:00:00Z`: `young` is empty and none of the 38 crates are yanked.

`webpki-roots` is not in `Cargo.lock`. `webpki-root-certs` and `rustls-native-certs` remain from the SDK lock.

## Merged feature graph

Command, on the integration snapshot, Linux rustc 1.94.0 aarch64:

`cargo tree --locked --offline -e features -i sqlx-core`

Selected `sqlx-core` features: `_rt-tokio`, `_tls-rustls`, `_tls-rustls-ring-no-roots`, `crc`, `migrate`, `default`, `rustls`, `sha2`, `tokio`, `tokio-stream`. `oo-connectors` enables `_tls-rustls-ring-no-roots`. `oo-pipeline` depends on `oo-connectors` default and adds no TLS feature. Forbid scan of that tree exit 0.

| Check | Exit |
|---|---|
| `cargo tree -i webpki-roots` | 101, package ID unmatched, empty output |
| `cargo tree -p oo-connectors -i webpki-roots` | 101 |
| `cargo tree -i rustls-native-certs` | 0 |

`rustls-native-certs` 0.8.4 is reached through `rustls-platform-verifier` 0.7.0 into `oo-sdk`, `reqwest`, and `oo-cli`. It is not a selected `sqlx-core` feature.

`cargo tree -p oo-connectors -i sqlx-macros` printed `warning: nothing to print` and exited 0. `sqlx-macros` 0.9.0 is in the lock because registry `sqlx` depends on it. It is not in the selected `oo-connectors` tree.

## Compile-time guard

`sqlx-core/src/lib.rs` rejects, for every feature set, `webpki-roots`, `rustls-native-certs`, `native-tls`, `_tls-rustls-ring-webpki`, `_tls-rustls-ring-native-roots`, `_tls-rustls-aws-lc-rs`, and `_tls-native-tls`, including an additive union with `_tls-rustls-ring-no-roots`.

Negative probes were separate crates under `/tmp` with their own `CARGO_TARGET_DIR`. They were not workspace members.

| Probe | Command result |
|---|---|
| `sqlx-core` features `_tls-rustls-ring-no-roots` + `_tls-rustls-ring-webpki` | Offline resolution missed `webpki-roots`. The online retry downloaded `webpki-roots` 1.0.9 into the probe target and failed at `compile_error!`. Recorded exit 101. |
| registry `sqlx` `=0.9.0` features `runtime-tokio`, `postgres`, `tls-rustls`, with the same path patch | Failed at the same `compile_error!`. Exit 101. |

The union did not load 119 roots and exit 0. `webpki-roots` from that probe was not written into the workspace lock.

## Commands and exits

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform `linux/arm64`, `RUSTUP_TOOLCHAIN=1.94.0`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. One own Rust container at a time. Global count included the already-running `oo-a080-msrv-f669cc94`. Host cargo was not run. Pull never, cap-drop ALL, no-new-privileges, no host network, no privileged mode, no socket, no HOME mount, no host port.

Baseline on immutable `78d106fc`, skeleton connector:

| Command | Exit |
|---|---|
| `cargo fetch --locked` | 0 |
| `cargo test -p oo-connectors --lib --doc --locked --offline` | 101. Cargo refuses to mix `--doc` with `--lib`. Container exit 101. |
| Separate `--lib` after that failure | 0. `0 passed` |
| Separate `--doc` | 0. `0 passed` |
| `cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings` | 0 |

Integration container on the extracted source. Its overall exit is 101 because clippy was missing and the SDK fixture was missing. The guard exits are the intended rejection.

| Command | Exit |
|---|---|
| `cargo fetch` (unlocked; writes the after-lock) | 0 |
| `cargo test -p oo-connectors --lib --locked --offline` | 0. 14 passed, 0.00s |
| `cargo test -p oo-connectors --doc --locked --offline` | 0. 0 passed |
| `cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings` | 1. `cargo-clippy` was not installed for 1.94.0 in that container |
| feature forbid scan | 0 |
| guard union | 101 |
| guard facade | 101 |
| `cargo test -p oo-sdk --lib --tests --locked --offline` | 101. Snapshot omitted `contracts/sdk/json-query` |
| container `docker-run.exit` | 101 |

Follow-up in `linux-live`, same toolchain, `contracts/sdk/json-query` copied into the snapshot, clippy component installed, own PostgreSQL 17.10 on `oo-a083-pgnet-f11d0c6c-51e6-4709-83cf-9301808e3098`:

| Command | Exit |
|---|---|
| `rustup component add clippy --toolchain 1.94.0` | 0 |
| `cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings` | 0 |
| `cargo test -p oo-sdk --lib --tests --locked --offline` | 0. 5 passed, 18 passed, 1 passed, 1 passed |
| `cargo test -p oo-connectors --test live_pg --locked --offline` | 0. 1 passed, 4.46s |
| container `docker-run.exit` | 0 |
| `live-worst.exit` | 0 |

`linux-live/phases/worst.exit` is still 101. That file was left on the shared target volume by the integration script. The live script writes `live-worst.exit` and does not replace `worst.exit`. The live result is `docker-run.exit` 0 and `live-worst.exit` 0.

`cargo-deny` was not run.

## Fixture

Own PostgreSQL 17.10, `server_version` `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num` 170010. Run `f11d0c6c-51e6-4709-83cf-9301808e3098`. The live test covered the legal reader, the side writer, env override, wrong CA, wrong host, and `PGOPTIONS` (`role=oo_src_reader` and `server_version=13.14`) without leaving the parent `PGOPTIONS` changed. In-test line was `sql_audit ROLLBACK=36,SELECT=840,START=36,WITH=48`.

Independent audit after `docker stop`, on the frozen log: timestamp-bounded records, `app=oo-pg-source` only, single-quoted literals stripped, first keyword counted, every alphanumeric token checked for write words. Secret scan counted fixture `*.pass` bodies, `forged-secret`, canary `CANARY-COMMENT-SECRET-a060`, `app=forged-app`, and `user=forged-user` in the log and in `linux-live/console.log`. Bodies were not recorded.

| Result | Value |
|---|---|
| Tagged statements | 960 |
| Verbs | `SELECT=840`, `START=36`, `WITH=48`, `ROLLBACK=36` |
| Disallowed first keywords | 0 |
| Flagged write tokens | 0 |
| Secret hits in the log | 0 |
| Secret hits in the live console | 0 |
| Log sha256 | `8985b98a354cc8913218c682ad1bb25e8072b99915aac7a518c56ef91bb05dc4` |
| Log bytes | 556175 |

Other statement verbs outside `app=oo-pg-source`: `SHOW=2`. Connection and shutdown lines use apps `psql`, `oo-pg-fixture`, and `[unknown]`. The `[unknown]` fatal lines are rejected connects before an application name is set. Raw log: `pg/postgresql.log`. Counts: `pg/sql-audit.json`.

## Scoped issues

- `webpki-root-certs` remains in the lock from the SDK, including the target-specific CDLA supply gap from a073. It is pending. It is not a reason this source tree was blocked, and the allowlist was not opened for it.
- `rustls-native-certs` in the selected workspace tree belongs to `rustls-platform-verifier` / `oo-sdk`. The selected SQLx root store is the empty store behind `_tls-rustls-ring-no-roots`.
- `whoami` 2.1.3 is `Apache-2.0 OR BSL-1.0 OR MIT`. BSL is an unused alternative on an already-allowed OR. The allowlist was not changed. `cargo-deny` was not re-run, so this is a recorded license string, not a deny pass.
- `stringprep` 0.1.5 and `unicode-properties` 0.1.4 publish `MIT/Apache-2.0`, which is not an SPDX expression. Same status: noted, deny not run, policy not edited.
- `sqlx-macros` is locked and not selected by `oo-connectors`.
- The path-stable patch diff does not reproduce a074 hash `f4b1aff1`.
- Baseline `--lib --doc` exit 101 and the first integration clippy/SDK exits are preserved above. The successful reruns do not replace them.
- Bounded reader, type conversion, import, publishing, Gateway, and CLI work were not done and were not replaced with mocks.

## Cleanup

Exact a083 containers, networks, volumes, and `/tmp/oo-a083-*` directories were removed after the log copy. No prune, no foreign deletion. Details in `cleanup.txt`.
