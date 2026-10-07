# Findings

Scope is the frozen exporter `62b4afecdd377fa94310f46467c48a01e61d14db` and the frozen producer `21cc96241a841b47dbbcb116549030e38a643ba0`. Product source was not edited. The earlier unit packet, including the a331 withhold written into the parent `REPORT.md`, was not rewritten.

## Verdict

The compiled chain matched the hand gold.

Complete `query-pg` stdout: six rows, `returned_rows` 6, `result_bytes` 162, `truncated` false. The CSV is `gold/amounts.csv`, 208 bytes, SHA-256 `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`. The sidecar records `decimal_count` 10, `int_count` 6, `null_count` 2, `formula_prefixed` 0, and `complete` true. The ten decimal cells keep coefficient and scale 4, including coefficient `0` for `0.0000` and the 38-digit wide coefficient. Nulls are `[[5,1],[5,2]]`. The CSV contains no apostrophe. `source_sha256` and `csv_sha256` match the file bytes. `export.source.jsonl` is a byte copy of the stdout.

Truncation: the same binary and the same SQL with `max_rows` 1. Summary `truncated` true, `returned_rows` 1, `result_bytes` 59, status `succeeded`. Exporter exit 1, stderr `query-result-export: partial`. Sidecar `complete` false and `partial` true. CSV is the header plus the first hand row, 85 bytes.

Formula boundary on that same complete stdout: columns `entry_id`, `amount`, `-wide`. The header field is `'-wide`. The file contains one apostrophe. `formula_prefixed` stays 0. The typed negatives `-12345678901234.5678` and `-0.0001` are not prefixed. The checker requires exactly one apostrophe in that file.

PostgreSQL text from `shop.entry` and from the target physical `numeric(18,4)` and `numeric(38,4)` columns matches those six display lines, including NULL.

## Smoke

`linux/smoke` on the exporter archive before the chain. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, Privileged false, network `oo-a333-eef831aa-net`. `cargo test` 22 + 3 exit 0. `cargo clippy --all-targets -- -D warnings` exit 0. `Cargo.lock` content SHA-256 `cc614d3de29ee73fb29ed7bfa9761102f9a3fe26d27244e8e76000016a791d22`. `cargo update` was not invoked.

## Chain builder

`linux/chain`. Same image, 6 GiB, 1 CPU, CapDrop ALL, Privileged false, OOMKilled false. Producer unit tests and producer Clippy were not repeated; a340 already recorded those exits at 0 on this commit. This run fetched, built the pipeline, builder, and gateway bins, ran `a327_live::live_numeric_proof`, built the exporter, ran the truncated query, and checked gold. Lock hash before and after matched.

`query-pg` SHA-256 `98d5cbba7b996023d7bac62ab7c88b85d695ae31e18aa289cbab1949f69a547a`. `query-result-export` SHA-256 `f119fe01894fd8f512d3fef605b772f6d99e40238c34ffccdc7fd6a1d24985d7`.

The live proof also recorded hidden exit 6 and before-analyze exit 5 in `outcome.json`. Those are the producer's own refusals. They are not a new export claim.

## Failed attempts kept

`linux/chain-failed-1`: builds and the live proof exited 0, then `chain-oracle.rs` failed to compile because `str::contains` does not accept `String` on this toolchain. Container removed. Cause file is in that directory.

`linux/chain-failed-2`: the retry found the audit file from the first proof. `query-pg` exited 7 with stderr `query-pg: audit` and the live test exited 101. An existing audit path is refused. The next `inside.sh` removes prior `audits/*.jsonl` before the proof. The console and `stop.json` in that directory are the run that failed.

## What this does not close

No ledger edit. No parent acceptance. No release. The U+FEFF text gold ran again only as a unit test in the smoke.
