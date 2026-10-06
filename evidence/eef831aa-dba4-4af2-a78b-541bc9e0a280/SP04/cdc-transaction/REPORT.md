# CDC transaction assembler

Session `eef831aa-dba4-4af2-a78b-541bc9e0a280`. Unit CDC-D01 / SP04-W01A.

This report records the bounded Rust assembler in `tools/cdc-transaction`. It does not accept CDC-D01. It does not close SP04. It does not certify a Debezium normalizer, a verified upstream commit LSN, P2, P11, a fenced target merge, an offset store, or an ACK.

## Product

Standalone Cargo workspace `tools/cdc-transaction`, edition 2024, rust-version 1.94.0. It is not a member of the root workspace. Dependencies are the locked pins `serde =1.0.229` and `serde_json =1.0.151`, copied from `tools/session-transition/Cargo.lock` with the package name changed. No `cargo update`. SHA-256 is the in-crate FIPS 180-4 function.

The contract is `tools/cdc-transaction/CONTRACT.md`, frozen before the implementation and corrected once while the Linux proof was still red. The correction is in FINDINGS: the 32 MiB budget counts opaque key and payload value bytes. The event-hash preimage is unchanged.

The library assembles an explicit normalized PostgreSQL CDC feed of `bind`, `barrier`, `begin`, `data`, `end`, and `rollback`. The CLI reads JSONL on stdin and writes JSONL on stdout. It takes no arguments. Exit 0 is a clean feed. Exit 3 is EOF with `resume_required` and no refusal. Exit 2 is a refusal. Exit 1 is usage or I/O.

Commit position is `epoch << 64 | commit_lsn` as a decimal string. The hand value is epoch 1 and LSN `16/B6E40`, position `18446744168199581248`. Attestations are fixed false: `upstream_commit_lsn_verified`, `cross_table_consistent`, `target_applied`, `ack_allowed`, `debezium_normalizer`, `p2_verified`, `p11_verified`. Progress keeps `applied` null.

## Base

Fixed commit `b859600c095e158a3af3017eb6561962eb780f9a`, tree `fce04e51c9b2ba1c344181246215a7e72739f682`. That commit is an ancestor of HEAD. `crates/oo-core/src/decimal.rs`, `crates/oo-core/src/rfc3339.rs`, `crates/oo-core/Cargo.toml`, root `Cargo.toml`, and root `Cargo.lock` are unchanged between that commit and HEAD. The Position overlay was compiled from a `git archive` of the fixed commit. The fixed archive does not contain `crates/oo-core/tests/cdc_d01_position.rs`. The overlay is evidence only.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, Rust 1.94.0. PostgreSQL was not started.

## Proof

Precode, before the product crate existed, on one 6 GiB / 1 CPU builder:

- `linux/precode/docker-run.exit` = 0
- probe stdout `msrv_smoke_ok active=16 txn_events=200000 payload=33554432 line=1048576 position=18446744168199581248`

Development attempts, preserved:

| attempt | docker exit | stop |
|---|---|---|
| linux/impl | 101 | `u64::try_from` matched as `Option`; `digits()` is not a `Result` |
| linux/impl-2 | 101 | `CARGO_BIN_EXE_cdc_transaction` unset; library compiled |
| linux/impl-3 | 101 | unsorted bind failed before count overflow; 200000 tiny events exceeded a preimage-byte budget |
| linux/impl-4 | 101 | tests passed; Clippy `question_mark`, `collapsible_if`, `manual_is_multiple_of` |
| linux/impl-5 | 101 | tests passed; Clippy on the binary |
| linux/impl-6 | 1 | tests and Clippy passed; `cargo fetch --locked -p oo-core` rejected `-p` |
| linux/impl-7 | 0 | tests, Clippy, and the Position overlay passed |

`linux/impl-7` host config: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a280-eef831aa-net`, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000, container `33b3d707da9fcc040fe286cffcd9bec3279da36bb5ea38ab5bbbc353817ee493`, removed as `oo-a280-impl-7`.

Test totals on impl-7: 5 library tests, 3 CLI tests, 19 hand-computed tests, 0 doctests. Clippy `--all-targets -- -D warnings` finished with exit 0. Position overlay, offline after a locked `--no-run` compile of `oo-core`:

- `frozen_positions_fit_oo_core_position`
- `max_epoch_and_lsn_fit_and_wider_values_do_not`
- `sample_timestamps_keep_the_stored_spelling`

`linux/review-1` repeated that proof on a fresh target volume `oo-a280-eef831aa-target-review`. docker exit 0. Same test totals. Position tests passed. Container `7fc76a253e4d7a95bff3532cf670cb835a133815a82395bb0338e8eaba788f17`, removed as `oo-a280-review-1`. Host config matches the 6 GiB / 1 CPU / cap-drop ALL shape. OOMKilled false.

Eight development containers were used, including precode. One review container rebuilt the crate. The manifest container is separate and does not rerun the tests. No host `cargo` or `rustc` ran beside a container. No second builder of this session ran at the same time.

## Inventory

`product.sha256` lists `tools/cdc-transaction`. `MANIFEST.sha256` lists this evidence directory except itself. Both are produced by the byte-copied a199 `sha256_manifest.rs` (`d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`). The host writes these files after the hash, so they are raw records and are not rows in `MANIFEST.sha256`: `linux/manifest/console.log`, `linux/manifest/docker-run.exit`, `linux/manifest/hostconfig.txt`, and `linux/manifest/container-removed.txt`.
