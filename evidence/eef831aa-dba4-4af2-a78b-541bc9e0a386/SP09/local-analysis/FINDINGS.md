# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a386`.

## Card status

The local tool now reads one verified `query-result-export`, checks lineage, and can write exact CSV, Parquet, Arrow IPC, an Excel workbook of text cells, a Chinese SVG, a checked font copy, and a manifest in one publish. DuckDB CLI 1.5.6 aggregates the Parquet file under one thread, a 256 MiB memory limit, extension auto-install and auto-load off, `allowed_directories` limited to the private temp directory, `enable_external_access` false, and `lock_configuration` true. Rust compares DuckDB’s decimal text with the digit-string groups before anything is published. A mismatch writes nothing.

`closes_sp09_w05` stays false. `audited` stays false. `reviewers_assigned` stays empty. The proposal does not edit `supply-chain/dependency-baseline.json`, root `Cargo.toml`, or root `Cargo.lock`. `crates/oo-analysis` was not edited. A seven-day security exception was not requested. `scan_recorded` is false because no cargo-audit or cargo-deny run was recorded.

`linux/chain/analysis-exits.txt` still records the earlier refusals (`parquet=3 xlsx=3 engine=3`). Those files were left in place. `linux/chain-full` is the run that produces the files.

## What the private chain showed

The live numeric proof and the frozen exporter produced this card’s export. The analysis binary then read `linux/chain-full/out/complete` and `linux/chain-full/audits/amounts.jsonl` from that same run. The fixture under `tools/local-analysis/tests/fixtures` is the a344 review copy used by unit tests. The chain input is the fresh stdout, whose digest differs from that fixture’s stdout. The exported CSV digest is `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`.

This run’s audit SHA-256 is `b0579351099153df2a5076c4744018f6fe3d71bbfb7dfe590aee3289e3307bb5`. The derived query id is `q_49b26b99a4752eca`. Role `oo_r_clerk`. Project `a327num`. Trust remains `local_operator_declared`. Authorization in the manifest is `not_claimed`.

DuckDB’s grouped sums matched the exact decimal text, including scale 4 and `0.0000`. The null amount increments `null_values` and adds no group. The wide column stays on `rust_exact` and keeps the 38-digit values as text. Typed minus signs are not apostrophe-prefixed. The truncated export is written and its SVG starts with `不可引用`.

Excel cells are `write_string_with_format` with number format `@`. `examples/xlsx_scan` inflates the workbook and checks the XML. The chain recorded `xlsx_scan_ok`. The earlier proof’s `grep` of the zip bytes is in `linux/chain-full/failed-attempt-1` and is a harness miss, not a missing amount.

The amount SVG uses one relative `@font-face` for `NotoSansCJKsc-Regular.otf`. The only `http` URL is the SVG namespace. The copied font’s SHA-256 matches `native/font.json`.

Cancel before work returns exit 1 and leaves the output directory empty. `pinned_duckdb_matches_the_exact_sum_and_cancel_kills_the_child` passed in `linux/full-test` with `OO_A386_REQUIRE_DUCKDB=1`: after 200 ms the stop file is written and `prove_native_cancel` kills the CLI running `range(250000)` cross `range(250000)`. A DuckDB request with no CLI returns exit 3, token `engine`, and does not replace `keep.txt`.

## Checks

`linux/full-test/clippy.exit` is 0 after `rustup component add clippy` inside the private builder. `linux/chain-full/clippy.exit` is 0 for `--all-targets -D warnings`, including `examples/xlsx_scan`. `linux/test/clippy.exit` remains 1 from the image that had no Clippy component.

`linux/full-test/failed-attempt-2` records the `ChunkReader` bound: the Parquet reader accepts `std::fs::File`, and the unit test now reads a temp file.

## Boundaries held

Write paths are `tools/local-analysis/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386/**`. No root manifest change, no `cargo update`, no host `cargo`, no image pull, no push, no release, and no customer call. The child lock was generated once in the private builder. The chain and the unit tests then used `--locked --offline`. One Rust builder at a time, 6 GiB, 1 CPU. One PostgreSQL 17.10, 1 GiB, 1 CPU, no host port. Network was the private bridge `oo-a386-eef831aa-net`.
