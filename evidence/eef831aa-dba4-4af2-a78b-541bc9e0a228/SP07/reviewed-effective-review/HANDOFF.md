# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a228`. Reviewed commit `9daeaa05e34c154391a757fc78fb47f09c954c6a`. This handoff does not accept M1-D18A. It does not check a parent box. It does not release. M1-D18 publication stays open.

## Verdict

`APPROVE_LIMITED` for the reviewed-effective compiler and its two-role consumption at that commit. Evidence is this directory. Author proof read at `865fb571c8bd662e48244f66233ee56adf08c07f`.

## Read first

- `REPORT.md` — smoke, two preserved overlay failures, and the green overlay.
- `FINDINGS.md` — bindings that held, and the limits that stay open.
- `linux/preoverlay/phases/` — author lib and integration tests, exit 0, before the harness existed.
- `linux/overlay-attempt1/` — harness panic on a missing `contact` fingerprint, exit 101.
- `linux/overlay-attempt2/` — tests passed, Clippy rejected the harness loop, exit 101.
- `linux/overlay/phases/` — green independent tests and Clippy, exit 0.
- `origin/recipes.txt` — a204, a200, and a199 blobs and the edits.
- `CLEANUP.txt` — a228 volumes, network, and private tmp removed. Images kept.

## Reproduce

One Linux arm64 Rust 1.94 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--memory 6g`, `--cpus 1`, private cargo and target volumes. Archive `9daeaa05e34c154391a757fc78fb47f09c954c6a`. Move the extracted `rust-toolchain.toml` aside. No PostgreSQL.

Smoke, harness absent:

```text
cargo test --locked --offline -p oo-builder --lib reviewed_effective -- --test-threads=1
cargo test --locked --offline -p oo-builder --test reviewed_effective -- --test-threads=1
```

Overlay, harness copied onto a fresh archive:

```text
cargo test --locked --offline -p oo-builder --test reviewed_effective_a228_review -- --test-threads=1 --nocapture
cargo clippy --locked --offline -p oo-builder --lib --bins --test reviewed_effective --test reviewed_effective_a228_review -- -D warnings
```

Green transcripts: `linux/preoverlay/console.log` and `linux/overlay/console.log`. `smoke.exit` is 0. `overlay.exit` is 0. Container ids are in each `hostconfig.txt`.

## Inventory

`linux/hash-attempt1` is container `77a83a21d09db00660ec3c5a15b4c5c444fb0ed96952f0650c5fd6b8680ce77e`. `docker-exec.exit` is 126. `/tmp/sha256` was not executable. No `SHA256SUMS` was written. The rerun compiles to `/usr/local/bin/sha256` inside the container.

`linux/hash/launch.sh` compiles `tools/sha256.rs` with the pinned image and writes `SHA256SUMS` plus `SHA256SUMS.sha256`. The tool skips those two names. `linux/hash/hostconfig.txt` is written before the digest walk, so the inventory includes it. The script then removes only the `oo-a228-hash` container.

## Still open

Authentication, grants, row-security proof, semantic publication, and gates G1–G13. `Cargo.toml` description still says the crate does not classify. The catalog hash is not exported, so this fixture reuses the locked shop catalog bytes. No dependency was added.

## Cleanup

`oo-a228-smoke` and `oo-a228-overlay` were removed by their launch scripts. `CLEANUP.txt` records removal of volumes `oo-a228-eef831aa-cargo` and `oo-a228-eef831aa-target`, network `oo-a228-eef831aa-net`, and `/tmp/oo-a228-eef831aa-dba4-4af2-a78b-541bc9e0a228` at `2026-10-05T22:17:18Z`. The script checks owner `a228` and this session. It does not prune. Pinned images stay. `linux/hash/launch.sh` removes its own container after the inventory.
