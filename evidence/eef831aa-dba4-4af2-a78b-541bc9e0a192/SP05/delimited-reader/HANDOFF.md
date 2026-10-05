# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a192`. Source commit `671507b229ad7d1bc6a2e948b5b74d6d7252393f`. Parent `65169f374c665d8efdd62a8bcd47317c38a1359e`. Baseline `15d23fe9f2cbc74e3f3278d70d0f56390568126e`.

## Read first

- `contracts/connectors/delimited-reader-v1.md`
- `crates/oo-connectors/src/delimited.rs`
- `crates/oo-connectors/tests/delimited.rs`
- `linux/focused-4/` for the 25 reader tests, the 64 library tests, clippy, and the fixture oracle
- `linux/preedit/` and `linux/preedit-2/` for the failed archive and the green baseline
- `linux/focused/`, `linux/focused-2/`, and `linux/focused-3/` for the preserved compile and clippy failures
- `oracle/` for the CSV and TSV bytes, preimages, lineage, and Rust SHA-256 dumps
- `source/blobs/` matches the source commit. `source/commit.txt` is the hash. `source/blob-sha256.txt` maps each blob to its path

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, `--locked --offline` after a warm cargo home. The workspace `rust-toolchain.toml` still says channel `1.98.1`. The override has to stay `1.94.0`.

```text
cargo test --locked --offline -p oo-connectors --lib
cargo test --locked --offline -p oo-connectors --test delimited
cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings
```

The reader tests hash with the Rust oracle in `tests/delimited.rs`. They do not call `sha256sum`. They do not need PostgreSQL. `OO_A192_ORACLE_DIR` receives the fixture bytes, preimages, lineage, and digests when set. A fresh container does not keep the clippy component. Install clippy inside the container that runs clippy.

## Review notes

`DelimitedComplete` proves a parse: EOF, profile coverage, and the object hash. `admits_import` and `published` stay false. It is not `RunComplete` and it does not admit a loader. Decimal projection uses `oo_core::ExactDecimal` and the public Arrow `Decimal128Array` constructor.

Cancel and the deadline are checked before each `read`. The contract section "Blocking read" records that a caller can still block inside `Read::read`. The hold test shows that case: cancel during the blocked call does not abort it, `reads` becomes 1, and `finish` returns `cancelled`.

Own containers were removed at the end of each launch script. `CLEANUP.txt` records removal of network `oo-a192-eef831aa-net`, volumes `oo-a192-eef831aa-cargo` and `oo-a192-eef831aa-target`, and the `/tmp/oo-a192-*` archives. The pinned Rust image was left in place. `docker prune` was not run. Foreign containers, including `oo-a190-pg`, were left untouched. `MANIFEST.sha256` is the SHA-256 of every file in this directory except itself. The author review is `FINDINGS.md`. A separate fixed review has not run.
