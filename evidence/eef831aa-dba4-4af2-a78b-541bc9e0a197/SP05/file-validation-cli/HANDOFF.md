# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a197`. Source commit `59be14baab1d5f30abaa2ce8cf67d607a2e4417f`. Parent `ef6c4fc7da03342f900146e90ff692fe742d1ba4`.

The delimited reader commit `671507b229ad7d1bc6a2e948b5b74d6d7252393f` is author-only. This command integrates its public constructors. Shipping the binary does not accept that library. The next step is a fixed review of this command, and that review has to verify the integration rather than treat the library as already accepted.

## Read first

- `contracts/pipeline/validate-file-cli-v1.md`
- `crates/oo-pipeline/src/bin/validate-file.rs`
- `crates/oo-pipeline/tests/validate_file_cli.rs`
- `linux/focused-3/` for the 11 process tests, clippy, rustfmt `--check`, and the fixture oracle
- `linux/preedit/` for the immutable pipeline smoke before these files existed
- `linux/focused/` and `linux/focused-2/` for the preserved compile and clippy failures
- `oracle/` for the CSV, TSV, and JSONL bytes, preimages, SHA-256 dumps, and success documents
- `source/blobs/` matches the source commit. `source/commit.txt` is the hash. `source/blob-sha256.txt` maps each blob to its path
- `FINDINGS.md` is the author review

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, `--locked --offline` after a warm cargo home. The workspace `rust-toolchain.toml` still says channel `1.98.1`. The override has to stay `1.94.0`. A fresh container does not keep clippy or rustfmt. Install them inside the container that runs them.

```text
cargo test --locked --offline -p oo-pipeline --test validate_file_cli
cargo clippy --locked --offline -p oo-pipeline --bin validate-file --test validate_file_cli -- -D warnings
rustfmt --edition 2024 --check crates/oo-pipeline/src/bin/validate-file.rs crates/oo-pipeline/tests/validate_file_cli.rs
```

The process tests hash with the Rust oracle in `tests/validate_file_cli.rs`. They do not call `sha256sum` for the object hash. They do not need PostgreSQL. `OO_A197_ORACLE_DIR` receives the fixture bytes, preimages, digests, and stdout when set. Do not run the live PostgreSQL pipeline tests for this slice.

## Review notes

Success stdout is one `oo.pipeline.validate-file-result/v1` object after `finish`. `admits_import` and `published` are false. `arrow_ipc` and `executes_native` are false. The document has no row payloads. Exit 0 is the only success code. Config, read, parse, budget, hash, and cancel have the exits and facts in the contract.

The input is a trusted local file. Cancel and the deadline are checked before each blocking `read`, not during it. The command does not claim a worker, an isolation boundary, or a public authentication decision.

`oo-pipeline` has no direct `arrow-array` dependency, and this task did not change a manifest. The decimal check is `oo_core::ExactDecimal` plus the `inexact_decimal` fact and the profile preimage. `Decimal128Array` cells were not inspected.

Own containers were removed at the end of each launch script. `CLEANUP.txt` records removal of network `oo-a197-eef831aa-net`, volumes `oo-a197-eef831aa-cargo` and `oo-a197-eef831aa-target`, and `/tmp/oo-a197-eef831aa-dba4-4af2-a78b-541bc9e0a197`. The pinned Rust image was left in place. `docker prune` was not run. Foreign containers were left untouched. `MANIFEST.sha256` is the SHA-256 of every file in this directory except itself. A separate fixed review has not run.
