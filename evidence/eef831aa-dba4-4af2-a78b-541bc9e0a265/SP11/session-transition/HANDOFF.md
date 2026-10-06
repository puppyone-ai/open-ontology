# Handoff

Verdict: the pure session-transition reducer passed its frozen contract tests and Clippy on Linux arm64 Rust 1.94. This is the unit proof for `tools/session-transition`. The parent ledger was not edited. This file does not accept RT-D01 for the parent, does not close SP11, and does not certify a physical release.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a265`. Fixed base `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`. Owned paths: `tools/session-transition/**` and this evidence directory.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `tools/session-transition/CONTRACT.md` and `source/blobs/`
- `linux/precode/` for the MSRV smoke before product Rust
- `linux/impl-4/` for the green development run
- `linux/review-1/` for the independent fresh-target run
- `linux/impl/`, `linux/impl-2/`, and `linux/impl-3/` for preserved failures
- `CLEANUP.txt`, `MANIFEST.sha256`, `tools/sha256_manifest.rs`, `recipe-origin.txt`

## Decisive exits

`linux/impl-4`: `fetch.exit` 0, `test.exit` 0, `clippy.exit` 0, `docker-run.exit` 0. Container `22d44184b738f387a8d5ae2b4b46516a9a84310f01e6387e8fe77633c76dac15`.

`linux/review-1`: `lock.exit` `present`, `fetch.exit` 0, `test.exit` 0, `clippy-component.exit` 0, `clippy.exit` 0, `docker-run.exit` 0. Container `903956deba937579d8b19c95ecdfe1438878ebbd2bf5573058249b2103e8a5fa`. Target volume was `oo-a265-eef831aa-target-review`.

Both `test.stdout` files report 1 lib test, 2 CLI tests, and 12 handcomputed tests passed, 0 failed. Clippy stdout is empty.

`linux/precode`: `docker-run.exit` 0, probe stdout `msrv_smoke_ok idle=1800000 absolute=28800000 turn=600000 tools=60`. Container `80d4c757439035260f1c1dae51d01df0798c2d1b5ef88b20dc6070e3573ca5b4`.

## Preserved attempts

- `linux/impl`: `cp -a` permission denied, `docker-run.exit` 1. Container `a37c3a10614db13d1252e00e36019d777060a3d69a32b0015c2d4cbb582811b9`.
- `linux/impl-2`: missing `CARGO_BIN_EXE_session_transition` and four Clippy errors, `docker-run.exit` 101. Container `715297750fd77addd970b25734bcf73b15db2ed343fa1e68ecc7f35710a5fef9`.
- `linux/impl-3`: tests passed, unused `now` under `-D warnings`, `test.exit` 0, `clippy.exit` 101, `docker-run.exit` 101. Container `916daba2fe3d50a87cc0b07cb196ba9d5b9b78864aba9e71e4f2a7847ff64e46`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. Copy the crate with `cp -R` onto a writable work directory. Mount the source tree read-only. Keep `Cargo.lock`. One builder, 6 GiB, 1 CPU. Private cargo and target volumes. Network is required once in a fresh container so `rustup component add clippy --toolchain 1.94.0` can download. Tests and Clippy then use `--locked --offline`.

```text
cargo fetch --locked
cargo test --locked --offline -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

`source/blobs` hashes match `linux/review-1/source-sha256.txt`. `Cargo.lock` SHA-256 is `9cb340c0cbbc4b0cb561c05796e72702d0714eee16b7515f0e2f3600d58c6adc`.

## Manifest

`tools/sha256_manifest.rs` is a byte copy of the a204 tool. SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. It is std-only Safe Rust. It checks the empty digest and `abc`, refuses symlinks, and writes `MANIFEST.sha256` for every regular file except that manifest. `linux/manifest/launch.sh` compiles it with `rustc --edition 2024 -D warnings` to `/usr/local/bin/sha256_manifest`. `/tmp` inside the container is `noexec`. The first stdout is copied to `linux/manifest/console.log` after that process exits. A second run rewrites `MANIFEST.sha256` after `console.log`, `hostconfig.txt`, and the exit files are closed. That second stdout stays outside this directory.

## Cleanup

`CLEANUP.txt` records removal of network `oo-a265-eef831aa-net`, volumes `oo-a265-eef831aa-cargo`, `oo-a265-eef831aa-target`, and `oo-a265-eef831aa-target-review`, and `/tmp/oo-a265-eef831aa-dba4-4af2-a78b-541bc9e0a265`. The pinned Rust image stays. `docker prune` was not run. PostgreSQL was not started.

## Scope left open

Durable database CAS, authentication, a real runtime, and a physical S8 absence check are outside this crate. The dedup journal holds 1024 events and fails closed when a new id would exceed that cap.
