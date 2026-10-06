# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a272`. Reviewed commit `b0f7da4771e3c3625a45329c721e174f4380571c`, tree `c746c6413ebf15a44498bb25ba6522f5cbbbafc9`. Fixed archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`, tree `7e83a381c8a90c5f61f3ce73947eb7ac441381e9`. Repo HEAD recorded at the proof launch is `eb9fdf46313d8a1d3cc4e25eb1fa09be28e3842e`.

This directory is the independent review of the pure session-transition reducer and CLI. The author session `a265` `linux/review-1` is not this review. The parent ledger was not edited. This file does not accept RT-D01 for the parent, does not close SP11, and does not certify a physical release.

The approval covers the reducer and CLI behavior exercised by the raw exits below. The CAS compare uses the numbers in the supplied state. `database_used` stays false. The identity tuple is caller-supplied. `auth_provider_used` stays false. S8 `actual_absence` is caller-declared. `physical_release_certified` stays false. A full journal of 1024 entries refuses an unjournaled complete release as `dedup_exhausted`. A closed state admits an identical replay and refuses the other events the overlay sent.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/` and `source/commit.txt`
- `overlay/a272_handcomputed.rs`
- `oracle/registry-triples.txt` and `tools/lock_subset.rs`
- `linux/proof/` for the 15-test smoke, the overlay, Clippy, and the preserved parser failure
- `linux/lockcheck/` for the 11-triple subset check
- `CLEANUP.txt`, `MANIFEST.sha256`, `tools/sha256_manifest.rs`, `recipe-origin.txt`

## Decisive exits

`linux/proof` container `36a95d97302e8eb99b08796de4c941fb1af2a0a9cc811876b86e280730d7b8b2`. `docker-run.exit` 2 because `lock-subset.exit` is 1. `smoke-test.exit` 0, `smoke-count.txt` 15, `test.exit` 0 with 11 passed and 0 failed, `clippy.exit` 0, `product-unchanged.exit` 0. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a272-eef831aa-net`.

`linux/lockcheck` container `1abf212136a4115c291ba5736b0f22109cfab82d179135e2accc2b8a51023631`. `docker-run.exit` 0, `lock-subset.exit` 0, `lock-compare.exit` 0, `lock-summary.exit` 0. NetworkMode `none`. Same memory, CPU, and CapDrop.

Both `capacity.txt` files recorded `own=0 global=1 host_cargo=0` before start. PostgreSQL was not started.

## Preserved attempt

The proof lock phase is the preserved failure. `linux/proof/lock-subset.stderr` names `oo-analysis 0.1.0`. `lock-triples.txt` from that container is empty. Do not reread `docker-run.exit` 2 as a product test failure. The product commands in that container exited 0, and the corrected subset check is `linux/lockcheck`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU. `git archive 9951cda7f128e30f1f6ce02e409d542d20a9e4fb`. Copy with `cp -R` onto a writable work directory. Mount the archive read-only. Keep the crate `Cargo.lock`. Do not copy that lock back onto the product tree.

```text
cargo test --locked --offline -- --test-threads=1
```

Copy `overlay/a272_handcomputed.rs` only after that smoke. Then:

```text
cargo test --test a272_handcomputed --locked --offline -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

A fresh cargo volume needs one online `cargo fetch --locked` before the offline commands. `linux/proof/fetch-offline.exit` is 101 and `fetch.exit` is 0 for that reason. Clippy on this image needed `rustup component add clippy` once, which uses the network.

`source/blobs` hashes match `linux/proof/source-sha256.txt`. Crate `Cargo.lock` SHA-256 is `9cb340c0cbbc4b0cb561c05796e72702d0714eee16b7515f0e2f3600d58c6adc`.

## Manifest

`tools/sha256_manifest.rs` is a byte copy of the a204 tool. SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. It is std-only Safe Rust. It checks the empty digest and `abc`, refuses symlinks, and writes `MANIFEST.sha256` for every regular file except that output. `linux/manifest/launch.sh` compiles it with `rustc --edition 2024 -D warnings` to `/usr/local/bin/sha256_manifest` because `/tmp` in the container is `noexec`.

The launch script runs two containers, one after the other. The first stdout is copied to `linux/manifest/console.log` after that process exits, and `docker-run.exit` plus `hostconfig.txt` are written then. The second container rewrites `MANIFEST.sha256` so those files are in the inventory. The second stdout stays outside this directory. `linux/manifest/docker-run.exit` is the first container. The script's own status is the refresh container. Network `none`, 6 GiB, 1 CPU, CapDrop `["ALL"]`.

## Cleanup

`CLEANUP.txt` records removal of network `oo-a272-eef831aa-net`, volumes `oo-a272-eef831aa-cargo` and `oo-a272-eef831aa-target`, and `/tmp/oo-a272-eef831aa-dba4-4af2-a78b-541bc9e0a272`, including the host precompute locks. Each of those removals exited 0. The pinned Rust image stays. `docker prune` was not run. Foreign owners were not touched. PostgreSQL was not started. `linux/manifest/launch.sh` removes its outside stdout directory when the refresh container exits 0.

## Scope left open

Durable database CAS, authentication, a runtime, kernel work, and a physical S8 absence check are outside this crate. The dedup journal holds 1024 events and fails closed when a new id would exceed that cap. Parent RT-D01 stays unaccepted.
