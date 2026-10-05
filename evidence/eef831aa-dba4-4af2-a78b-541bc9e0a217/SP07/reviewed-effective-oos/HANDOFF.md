# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a217`. Fixed archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`. This handoff does not accept M1-D18A. It does not check a parent box. It does not release. M1-D18 publication stays open.

## Commit

Product `9daeaa05e34c154391a757fc78fb47f09c954c6a` on parent `02b84f4bf731f6a24dfc6aece454d7e875d86938`. The commit is only:

- `contracts/builder/reviewed-effective-oos-v1.md` blob `0a219892be7bb9643a29bd2c8022aa99071e33b5`, SHA-256 `81c06d4d27c7f2d21790a4f5f9fec70dcf4bdb5223416e7f4e501b6b80c6fc10`
- `crates/oo-builder/src/bin/reviewed-effective-oos.rs` blob `30a171fc617616d91912c84f9ee1d846388f5b4a`, SHA-256 `e466c12cdb603ce3e18483bb71e4dde5da59cd113200e71fb3d50d8b2afdd4fe`
- `crates/oo-builder/src/lib.rs` blob `c3a09422c79727ac3413ab8082c0c050f673eea4`, SHA-256 `b205d2104ecf5cdca6cf2eb2e9672d2e72727a8c78920d695fb2e939994d9b9a`
- `crates/oo-builder/src/reviewed_effective.rs` blob `7345b036685401d543e174a45a414e0e0db19613`, SHA-256 `189e8fb5e0d19e218ecdda625d8006e8b28fc295765f21106235aebb1e1daf4a`
- `crates/oo-builder/tests/reviewed_effective.rs` blob `4a1033eb0fe914b96286a70f0224c04bc93d25f2`, SHA-256 `6b954b0be9dc2bcfcfdbad3fef44043cd19d74ef939c8f6f298db2e1c3b3612d`

`lib.rs` versus the fixed commit is 14 insertions and 5 deletions. The green container compiled those bytes. `source/product/` matches the commit.

## Read first

- `REPORT.md` — smoke, failed overlays, and the green commands.
- `FINDINGS.md` — seal, decisions, the reject-merge correction, and open seams.
- `REVIEW.md` — same-session review of the frozen scope. Not an independent review.
- `linux/smoke/phases/` — pre-edit boundary tests, exit 0.
- `linux/smoke-attempt1/` — fixture omission, exit 101.
- `linux/overlay/phases/` — green lib test, CLI test, and clippy, exit 0.
- `linux/overlay-attempt1/` and `linux/overlay-attempt2/` — failed overlays. Do not delete them.
- `origin/recipes.txt` — a204, a200, and a199 recipe blobs and the edits.

## Reproduce

One Linux arm64 Rust 1.94 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--memory 6g`, `--cpus 1`, private cargo and target volumes. Archive `c2db9c430b822a824a1062b6c3c7345924df7dd8` plus the two `review_pg_cli` catalog fixtures, then copy the five owned files. Move the extracted `rust-toolchain.toml` aside. No PostgreSQL.

```text
cargo test --locked --offline -p oo-builder --lib reviewed_effective -- --test-threads=1
cargo test --locked --offline -p oo-builder --test reviewed_effective -- --test-threads=1
cargo clippy --locked --offline -p oo-builder --lib --bins --test reviewed_effective -- -D warnings
```

The green transcript is `linux/overlay/console.log`. `overlay.exit` is 0.

## Still open

Authentication, grants, row-security proof, semantic publication, and gates G1–G13. Column ACL for a second table and `reltuples` / `ANALYZE` are other seams. `Cargo.toml` description still says the crate does not classify. No dependency was added.

## Cleanup

`oo-a217-smoke` and `oo-a217-overlay` were removed by their launch scripts. `CLEANUP.txt` records removal of volumes `oo-a217-eef831aa-cargo` and `oo-a217-eef831aa-target`, network `oo-a217-eef831aa-net`, and `/tmp/oo-a217-eef831aa-dba4-4af2-a78b-541bc9e0a217`. The script checks owner `a217` and this session. It does not prune. Pinned images stay.
