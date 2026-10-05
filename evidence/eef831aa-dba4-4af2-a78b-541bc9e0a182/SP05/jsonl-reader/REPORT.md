# a182 bounded JSONL source reader

Slot B added `oo.source.jsonl/v1` to `oo-connectors`. The reader pulls flat JSON objects from any `std::io::Read`, projects an explicit profile into series-60 `RecordBatch` values, and mints `JsonlComplete` only after a real EOF, full selected coverage, and a matching whole-object SHA-256.

Source commit `47743688d45ec7a001e796725febc5357b7e20bd` on parent `ce040593f6af6a61a5c3e29abd494b1a863f32c5`. Fixed base `03512f91324ac33e772e9702fe806e8e04037ec4` is an ancestor of that parent. The commit contains only:

- `crates/oo-connectors/src/jsonl.rs`
- `crates/oo-connectors/src/lib.rs` (module and re-export)
- `crates/oo-connectors/tests/jsonl_reader.rs`
- `contracts/source/jsonl-reader-v1.md`

Those four blobs match the archive hashed in `linux/focused-6/source-sha256.txt`. No manifest, lock, dependency, PostgreSQL driver, snapshot, core, control, gateway, builder, pipeline, arch-gate, spec, or ledger file was edited.

## Behavior

The profile fixes column order, names, nullability, and the logical type (`utf8`, `bool`, `int64`, `json_text`). The reader does not infer a format or a semantic class. Supported projections are nullable UTF-8, bool, and int64, plus a declared `json_text` column that stores the original value bytes. JSON numbers for `int64` must be an optional minus and digits and must fit in `i64`. `oo_core::ExactDecimal::parse_json_number` is the exactness gate. Fractions, exponents, overflow, and type mismatches fail the pull. Duplicate keys, unknown keys, missing required keys, and bad rows fail the pull. The object hash is updated from raw bytes before parse and compared only at EOF. `JsonlItem::Exhausted` has no boolean EOF field. `finish` is the only constructor of `JsonlComplete`. `admits_import` and `published` are false. The header says Arrow IPC and native execution are false.

## Linux Rust 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, Rust `1.94.0` (`4a4ef493e 2026-03-02`). The runs used private network `oo-a182-eef831aa-net`, cargo volume `oo-a182-eef831aa-cargo`, and target volume `oo-a182-eef831aa-target`. Cap drop `ALL`, no new privileges, memory 8g, no host network, no PostgreSQL. `CLEANUP.txt` records removal of that network, those volumes, and the task `/tmp` archives after the logs were stored. The pinned image remains. `docker prune` was not run.

| Run | Result |
|---|---|
| `linux/preedit` | Exit 101. Archive omitted `contracts/`, so `empty-catalog.json` was missing. Log preserved. |
| `linux/preedit-2` | `cargo test -p oo-connectors --lib` exit 0. 64 passed. `absence.exit` 0 (`jsonl.rs` absent). `docker-run.exit` 0. |
| `linux/focused` through `linux/focused-5` | Compile and clippy failures preserved. Final product tests were not green there. |
| `linux/focused-6` | `--test jsonl_reader` 11 passed. `--lib` 64 passed. `cargo clippy -p oo-connectors --all-targets -- -D warnings` exit 0 (incremental). |
| `linux/focused-7` | `cargo clean -p oo-connectors` removed 1461 files. Clippy was not installed in that fresh container. Exit 1. Preserved. |
| `linux/focused-8` | `rustup component add clippy` exit 0. Clean-tree `cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings` exit 0. `Finished dev profile` in 3.71s. |

Focused-6 fixture oracle, agreed by the test's `sha2` call site, `sha256sum` on the temp file, and `sha256sum` on stdin:

`0d156573993aad8c4060dd56f3ec57d61c211c6157ce3cd8f822c6598daf7898`

Dependency compile printed two pre-existing `sqlx-postgres` `unfulfilled_lint_expectations` warnings. They are outside this slice. The oo-connectors clippy command still exited 0.

## What this slice does not do

It does not import, publish, write Arrow IPC, sniff with DuckDB, execute a native payload, or open a file-hosting adapter. `JsonlComplete` is a parse receipt. Arrow IPC workers, multi-format readers, DuckDB sniff, native isolation, runsc, and the file loader stay open.
