# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a401`.

## Card status

At commit `e2732e323915a332a85f008c605b50c1f4d5e948`, the local tool reads one verified `query-result-export`, checks lineage, and can write exact CSV, Parquet, Arrow IPC, an Excel workbook of text cells, a Chinese SVG, a checked font copy, and a manifest in one publish. DuckDB CLI 1.5.6 aggregates the Parquet file under one thread, a 256 MiB memory limit, extension auto-install and auto-load off, `allowed_directories` limited to the private temp directory, `enable_external_access` false, and `lock_configuration` true. Rust compares DuckDB’s decimal text with the digit-string groups before anything is published.

`closes_sp09_w05` stays false. `audited` stays false. `reviewers_assigned` stays empty. The proposal does not edit `supply-chain/dependency-baseline.json`, root `Cargo.toml`, or root `Cargo.lock`. This review did not edit `tools/local-analysis` or `crates/oo-analysis`. A seven-day security exception was not requested.

## What this private chain showed

The live numeric proof and the frozen exporter produced this card’s export. The analysis binary then read `linux/proof/out/complete` and `linux/proof/audits/amounts.jsonl` from that same run. The exported CSV digest is `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`. This run’s stdout SHA-256 is `f1f1c1964c646475ff6a1a97363dc99c59a68abd4fb26d5dd2961b7ed3705b55`. This run’s audit SHA-256 is `ed4819d6c664163bb1aa68cb61355ee5e5c0efcca41603886787aeb789d544c5`. The derived query id is `q_f714069af89b93f6`. Role `oo_r_clerk`. Project `a327num`. Trust remains `local_operator_declared`. Authorization in the manifest is `not_claimed`.

DuckDB’s grouped sums matched the exact decimal text, including scale 4 and `0.0000`. The null amount increments `null_values` and adds no group. The wide column stays on `rust_exact`, writes Parquet, and keeps the 38-digit values as text. Typed minus signs are not apostrophe-prefixed. The header `-wide` is written as `'-wide`. The truncated export is written and its SVG description starts with `不可引用`.

Excel cells are text. `examples/xlsx_scan` inflates the workbook. The amount scan and the formula scan both recorded `xlsx_scan_ok`. The formula needle `'-wide` is present, so the shared string kept the apostrophe.

The amount SVG uses the checked Noto file. The only `http` URL is the SVG namespace. The copied font’s SHA-256 matches `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`.

Cancel before work returns exit 1 and leaves the output directory empty. `native_cancel_kills_the_child_and_leaves_no_temp_dir` passed in the focused test: the stop file is written and `prove_native_cancel` kills the CLI. A DuckDB request with no CLI returns exit 3, token `engine`, and leaves `keep.txt`. A 39-digit coefficient with parquet true returns `engine` and writes nothing. A tampered query id returns lineage and writes nothing.

## Dependency proposal

The recorded pins match the proposal. Ages at `2026-10-08T05:14:59Z`: arrow 1950236 seconds (22 days), parquet 1950176 seconds (22 days), rust_xlsxwriter 2065924 seconds (23 days), zip 17499351 seconds (202 days, dev-only), DuckDB CLI 833924 seconds (9 days), Noto CJK 143678385 seconds (1662 days). The seven-day floor is 604800 seconds. None are under it.

Crate MSRV values are 1.88 and 1.88.0. The tool `rust-version` and the builder toolchain are 1.94.0. DuckDB and the font are outside Cargo.

Separate gap: `scan_recorded` is false. `cargo audit` and `cargo deny` were not run. That absence is not an audit, not a signed release, and not a baseline edit. `exception` is null.

## Checks

`linux/smoke/smoke.exit` is 0 on Linux arm64 Rust 1.94 before the overlay. `linux/proof/focused-test-final.exit` is 0. `linux/proof/clippy.exit` is 0 after the overlay borrow fix and `rustup component add clippy`. The author whole matrix, producer unit tests, and producer Clippy were not repeated. `producer-units-not-repeated.txt` records that limit.

## Boundaries held

Writes are confined to `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a401/SP09/local-analysis-review/**`. No root manifest change, no `cargo update`, no host `cargo`, no image pull, no push, no release, and no customer call. One Rust builder at a time, 6 GiB, 1 CPU. One PostgreSQL 17.10, 1 GiB, 1 CPU, no host port. The proof network was the private bridge `oo-a401-eef831aa-net`. Password values were passed as file paths, not argv values.
