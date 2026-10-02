# HANDOFF a083 PostgreSQL source integration

Status: local commit ready for independent fixed-SHA review. Root Cargo.toml and Cargo.lock ownership is released at this terminal. The NFC dependency task is not started and is not assigned by this handoff.

Integration commit: `2980e85af22c3ade05272c66c7d2835b52730322` (tree `c027c4599cdebf701b21e97e51ded45b9067ca8e`). Its parent is `1868ffbca151c64a21efd6d1a09cb4806b1e95f3` (tree `66a4e486b0e552766218f35eeee3fa69a271d928`).

## Frozen inputs

| Item | SHA |
|---|---|
| Linux 1.94 baseline commit | `78d106fcb3e7e1705b617ed6df58933d85297e7b` |
| Baseline tree | `66010b8c215df498afbc585b5461b6668ae58cd9` |
| Baseline parent | `a45c13c0e231dcbf6c7b97050b9bfbe8a11bb571` |
| SDK delivery | `8efeb230afc6734a2e50eaca3f09fd78300a9c7d` |
| SDK stamp | `0b544cb066292221016899c9b5eceef84e984227` |
| Accepted correction | `4ecdbf714766baef3490a4f954701eaf19f245a0` |
| Correction parent | `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` |
| Original product base | `4213134d136ff50bd2f7d422f3f840e48e038b6d` |
| Merged Cargo.lock | `9b8f7e50d3b20042bb2d606f47276b76dd68f3568136343040fbea1ecd622057` |

`Cargo.toml`, `Cargo.lock`, and `crates/oo-connectors` are identical between the baseline commit and the commit parent. Product-pg stayed at `181df9b6ba030b68e3a48aa0f58e5c5f909bee55`.

## Graph result

Selected `sqlx-core` features from `cargo tree -e features -i sqlx-core`: `_rt-tokio`, `_tls-rustls`, `_tls-rustls-ring-no-roots`, `crc`, `migrate`, `default`, `rustls`, `sha2`, `tokio`, `tokio-stream`. `cargo tree -i webpki-roots` exit 101. `rustls-native-certs` is only `rustls-platform-verifier` to `oo-sdk` / `reqwest` / `oo-cli`.

Guard union (`_tls-rustls-ring-no-roots` + `_tls-rustls-ring-webpki`) exit 101 at `compile_error!`. Facade `sqlx` feature `tls-rustls` exit 101 at the same guard. Neither probe exited 0.

## Verification that passed on the rerun

Linux 1.94 `oo-connectors` tests: lib 14 passed, doc 0 passed, clippy `-D warnings` exit 0. SDK `--lib --tests` exit 0 (5, 18, 1, 1). Own PostgreSQL 17.10 `live_pg` 1 passed in 4.46s. Application-tag audit: 960 statements, `SELECT=840`, `START=36`, `WITH=48`, `ROLLBACK=36`, flagged writes 0, secret hits 0. Log sha256 `8985b98a354cc8913218c682ad1bb25e8072b99915aac7a518c56ef91bb05dc4`.

Preserved failures, explained in `REPORT.md`: baseline `--lib --doc` exit 101; first integration container exit 101 from missing clippy and a snapshot that omitted `contracts/sdk`. `linux-live/phases/worst.exit` remains that old 101. `live-worst.exit` and `docker-run.exit` for the live container are 0.

## Scoped issues left open

`webpki-root-certs` CDLA stays a pending SDK supply gap. `cargo-deny` was not run. `whoami` carries an unused BSL alternative. `stringprep` and `unicode-properties` use `MIT/Apache-2.0`. `sqlx-macros` is locked and not selected. The path-stable vendor diff does not reproduce a074 patch hash `f4b1aff1f2575a6b161a5cec37da26a4e097475aa1dcde3a9cec603766ac5f37`. Policy files were not loosened.

## Ownership

Released: root `Cargo.toml`, root `Cargo.lock`.

Not touched: `oo-builder`, `oo-pipeline`, `xtask`, the a080 spike, `ci/deny.toml`, the ledger, the plan. No push and no pull request.

a083 containers, networks, volumes, and `/tmp/oo-a083-*` were removed. No prune. `oo-a080-msrv-f669cc94` was left running. See `cleanup.txt`.
