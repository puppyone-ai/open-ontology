# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a278`. Reviewed commit `e76813f62150094f1b5b137d0e9d3ce291580149`, tree `a35f3cfdce10bbff42ffd32413bb92bad12a106c`. Fixed archive `7f500ff0a61a42ea606a281dd931099091a52ef6`, tree `5c000b8d8cf492fc99fb897aae39da673b3299fa`. Repo HEAD recorded at the proof launch is `aed25ac3651a9da8fc1839daaf582464a0935024`.

This directory is the independent review of the enum-history compiler and CLI. The author session `a270` `linux/review-1` is not this review. The parent ledger was not edited. This file does not accept SEM-D02 for the parent, does not close SP08, and does not certify a release.

The approval covers the compiler and CLI behavior exercised by the raw exits below. The count vector is caller-supplied. `database_used` stays false. `owner` and `review.by` are declared labels. `owner_authenticated` stays false. Declared coverage is echoed separately from the observed ratio. A zero observed total is undefined. Overlap refuses for future and historical pairs. History rows stay. Reserved names are not business classes. `u64` overflow refuses.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/` and `source/commit.txt`
- `overlay/a278_handcomputed.rs`
- `oracle/registry-triples.txt` and `tools/lock_subset.rs`
- `linux/proof/` for the preserved oracle failure
- `linux/proof-2/` for the 23-test smoke, the 11-test overlay, Clippy, and the 115-triple subset
- `CLEANUP.txt`, `MANIFEST.sha256`, `tools/sha256_manifest.rs`, `recipe-origin.txt`

## Decisive exits

`linux/proof-2` container `0a15a41b59667a15f7f0d2378b5753600a121b9eb4f70529602cb323e168303a`. `docker-run.exit` 0. `smoke-test.exit` 0, `smoke-count.txt` 23, `test.exit` 0 with 11 passed and 0 failed, `clippy.exit` 0, `lock-subset.exit` 0, `lock-compare.exit` 0, `product-unchanged.exit` 0. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a278-eef831aa-net`.

`linux/proof` container `8be6711282c90175cdb4c48a86ed10f30013c4af316635afab0188bbef91e8f0`. `docker-run.exit` 101. `smoke-test.exit` 0, `smoke-count.txt` 23, `clippy.exit` 0, `lock-subset.exit` 0, `test.exit` 101. Same memory, CPU, and CapDrop.

Both `capacity.txt` files recorded `own=0 global=0 host_cargo=0` before start. PostgreSQL was not started.

## Preserved attempt

The first overlay expected `reserved_class` where `validate_enum` returns `core_enum`. `linux/proof/test.stdout` names `reserved_classes_are_not_business`. `linux/proof/failed-a278_handcomputed.rs` is the overlay that ran. Do not reread `docker-run.exit` 101 as an author-suite failure. The author commands in that container exited 0. The corrected overlay is `linux/proof-2`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU. `git archive 7f500ff0a61a42ea606a281dd931099091a52ef6` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, and `tools/enum-history`. Copy with `cp -R` onto a writable work directory. Mount the archive read-only. Keep the crate `Cargo.lock`. Do not copy that lock back onto the product tree.

```text
cargo test --manifest-path tools/enum-history/Cargo.toml --locked --offline -- --test-threads=1
cargo test --manifest-path tools/enum-history/Cargo.toml --locked --offline --test a278_handcomputed -- --test-threads=1
cargo clippy --manifest-path tools/enum-history/Cargo.toml --locked --offline --all-targets -- -D warnings
```

The manifest pair is `linux/manifest/launch.sh`. It hashes this directory after the reports. Its refresh stdout stays outside the review directory.
