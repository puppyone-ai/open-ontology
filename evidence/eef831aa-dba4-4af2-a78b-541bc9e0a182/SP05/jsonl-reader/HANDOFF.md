# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a182`. Source commit `47743688d45ec7a001e796725febc5357b7e20bd`. Parent `ce040593f6af6a61a5c3e29abd494b1a863f32c5`. Fixed base `03512f91324ac33e772e9702fe806e8e04037ec4`.

## Read first

- `contracts/source/jsonl-reader-v1.md`
- `crates/oo-connectors/src/jsonl.rs`
- `crates/oo-connectors/tests/jsonl_reader.rs`
- `linux/focused-6/` for the 11 reader tests, the 64 library tests, and the fixture oracle
- `linux/focused-8/` for clippy after `cargo clean -p oo-connectors`
- `linux/preedit/` and `linux/preedit-2/` for the failed archive and the green baseline
- `oracle/fixture.sha256`, `oracle/profile-preimage.txt`, `oracle/config-preimage.txt`
- `source/blobs/` matches the source commit

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, `--locked --offline` after a warm cargo home.

```text
cargo test -p oo-connectors --test jsonl_reader -- --nocapture
cargo test -p oo-connectors --lib
rustup component add clippy
cargo clippy -p oo-connectors --all-targets -- -D warnings
```

The reader tests need `sha256sum` on `PATH`. They write temp files under `std::env::temp_dir()`. They do not need PostgreSQL. `OO_JSONL_ORACLE_DIR` receives the fixture and preimages when set.

## Review notes

`JsonlComplete` proves a parse: EOF, profile coverage, and the object hash. `admits_import` and `published` stay false. It is not `RunComplete` and it does not admit a loader. The public typed PostgreSQL batch API was not reused because its builder requires a PostgreSQL column plan and its decimal conversion is not a JSON entry point.

A fresh container does not keep the clippy component. `linux/focused-7` records that miss. Install clippy inside the container that runs clippy.

Own containers were removed at the end of each launch script. `CLEANUP.txt` records removal of network `oo-a182-eef831aa-net`, volumes `oo-a182-eef831aa-cargo` and `oo-a182-eef831aa-target`, and the `/tmp/oo-a182-eef831aa-*` archives. The labels were `oo.owner=a182` and no container was attached. The pinned Rust image was left in place. `docker prune` was not run. `MANIFEST.sha256` is the SHA-256 of every file in this directory except itself. The author review is `FINDINGS.md`. A separate fixed review has not run.
