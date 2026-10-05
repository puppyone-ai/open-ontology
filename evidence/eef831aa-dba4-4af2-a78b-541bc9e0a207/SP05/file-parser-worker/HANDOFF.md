# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a207`. Source commit `6bd2455849da2527973c136968d4549ecd98d770`. Parent `d20944017bda07ae8a7e95c8acfcfaebb829aa4a`. Fixed base `a661dcdf72ff91561ae6b7cb79916e582884d08a`.

## Read first

- `contracts/connectors/file-parser-worker-v1.md`
- `crates/oo-connectors/src/bin/oo-file-parser-worker.rs`
- `crates/oo-connectors/tests/file_parser_worker.rs`
- `build08/` for the green locked test, clippy, lock gate, image id, and host config
- `preedit/` for the green delimited and JSONL smoke before this slice
- `build/` through `build07/` for the preserved fetch, compile, and clippy failures
- `oracles/` for the crates.io JSON of `arrow-ipc` 60.0.0, `arrow-select` 60.0.0, `arrow-cmp` 60.0.0, and `flatbuffers` 25.12.19 and 25.2.10
- `source/blobs/` matches the source commit. `source/commit.txt` is the hash. `source/blob-sha256.txt` maps each blob

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, `--locked --offline` after the cargo home already holds the closure. The workspace `rust-toolchain.toml` channel is not 1.94.0. The override has to stay `1.94.0`.

```text
cargo test --locked --offline -p oo-connectors --test file_parser_worker -- --test-threads=8
cargo clippy --locked --offline -p oo-connectors --bin oo-file-parser-worker --test file_parser_worker -- -D warnings
```

The test hashes with `/usr/bin/sha256sum`. It does not start PostgreSQL. A fresh container does not keep clippy or rustfmt. Install those components inside the container that runs them. The binary env var is `CARGO_BIN_EXE_oo-file-parser-worker`.

## Review notes

`oo.source.file-parser-worker/v1` is a process, file, and IPC boundary around the already approved readers. The completion receipt is written only after the reader token. `admits_import` and `published` are false. `pg_run_complete` is false. It is not `PgRunComplete`.

`deadline_ms` is checked between reads. A blocked `read` keeps running until it returns or the parent kills the process. The test's kill and drop-guard cases are that parent. The binary does not install the watchdog.

The IPC cap refuses the sink before `StreamWriter` when column memory exceeds `batch_bytes + 65536`. It does not stop the public reader from allocating the `RecordBatch`.

The lock adds `arrow-ipc` 60.0.0, `arrow-select` 60.0.0, `arrow-cmp` 60.0.0, and `flatbuffers` 25.12.19. `flatbuffers` `25.2.10` is the caret floor in the `arrow-ipc` manifest. Cargo selected `25.12.19`. Both ages pass seven days. The supply-chain policy and the dependency baseline were not edited, and the new crates were not marked admitted.

`CLEANUP.txt` lists the containers, the network `oo-a207-a207preedit01-net`, and the volumes `oo-a207-a207preedit01-cargo` and `oo-a207-a207preedit01-target` removed after the logs were stored. The pinned Rust image was left in place. `docker prune` was not run. Foreign containers were left untouched. `MANIFEST.sha256` is the SHA-256 of every file in this directory except itself. The author review is `FINDINGS.md`. A separate fixed review has not run.
