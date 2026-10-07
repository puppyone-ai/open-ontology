# Findings

Scope is the frozen exporter `62b4afecdd377fa94310f46467c48a01e61d14db` and the frozen producer `21cc96241a841b47dbbcb116549030e38a643ba0`. Product source was not edited. a331 remains WITHHELD on its own commit. a333's packets were read and were not rewritten.

## Verdict

**LIMITED** for frozen child E44 / FILE-D04.

The compiled chain matched the independent hand gold. The exact-decimal clause is the coefficient and scale cell. It is not integer text renamed.

Parent SP05-W06, AT-FILE-05, AT-SEC-03, xlsx, HTTP authentication, download authorization, retention, deletion, the reaper, publish, and release stay open. No new unit. No ledger edit.

## Six rows

Complete `query-pg` stdout: six rows, `returned_rows` 6, `result_bytes` 162, `truncated` false, status `succeeded`. The CSV is `gold/amounts.csv`, 208 bytes, SHA-256 `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`. The sidecar records `decimal_count` 10, `int_count` 6, `null_count` 2, `formula_prefixed` 0, and `complete` true. The ten decimal cells keep scale 4. Two of them use the 38-digit coefficient `12345678901234567890123456789012345678`. Four are negative. Two use coefficient `0`, displayed `0.0000`. Nulls are `[[5,1],[5,2]]`. The CSV contains no apostrophe and no BOM. `source_sha256` `20623e7dc575b618223ac383ceb1b2e9a9e3f9efc1695c58ea2373bcaff4b3eb` matches `amounts.stdout`. `export.source.jsonl` is a byte copy of that stdout. `csv_sha256` matches the CSV file.

`shop.entry` and the target physical columns show the same six display lines, including NULL.

## Truncation

The same binary and the same SQL with `max_rows` 1. Summary `truncated` true, `returned_rows` 1, `result_bytes` 59, `source_bytes` 616, status `succeeded`. Exporter exit 1, stderr `query-result-export: partial`. Sidecar `complete` false, `partial` true, `truncated` true. `source_sha256` `4045364a9f9527c9502bcf39ac0942f90f566c5f49883c8d116f9ef4f5214177` matches `partial.stdout`, and the source copy matches those bytes. CSV is the header plus the first hand row, 85 bytes, SHA-256 `b3c7364819b209fc3746f349e22b501e5b223935edd925c40b267362c76d2304`.

## Formula boundary

Columns `entry_id`, `amount`, `-wide` on the complete stdout. The header field is `'-wide`. The file contains one apostrophe. `formula_prefixed` stays 0. The typed negatives `-12345678901234.5678`, `-1234567890123456789012345678901234.5678`, and `-0.0001` are not prefixed. CSV SHA-256 `10e18fe0ad9ae4a747bfea76156a21221b21c689e54804a2e2feb2f6c58e2079`.

## U+FEFF

`linux/smoke/test.log` records `test fef_before_equals_is_the_one_prefixed_gold_cell ... ok` inside the 22 API tests. That is the original unit case on the frozen exporter. It is not a new query-pg text receipt.

## Smoke and chain

`linux/smoke` before the chain. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, Privileged false, network `oo-a344-eef831aa-net`. `cargo test` 22 + 3 exit 0. `cargo clippy --all-targets -- -D warnings` exit 0. Exporter `Cargo.lock` content SHA-256 `cc614d3de29ee73fb29ed7bfa9761102f9a3fe26d27244e8e76000016a791d22`. `cargo update` was not invoked. Source hashes are `linux/smoke/source-inventory.txt`.

`linux/chain` used the same image, 6 GiB, 1 CPU, CapDrop ALL, Privileged false, OOMKilled false. Producer unit tests and producer Clippy were not repeated. This run fetched, built the pipeline, builder, and gateway bins, ran `a327_live::live_numeric_proof`, built the exporter, ran the truncated query, and checked gold. Lock hash before and after matched.

`query-pg` SHA-256 `342ac08c5a7cf402d350759f8c4d3d253ff6b3c574ccd58e334d613a516b0316`. `query-result-export` SHA-256 `f119fe01894fd8f512d3fef605b772f6d99e40238c34ffccdc7fd6a1d24985d7`.

The live proof also recorded hidden exit 6, before-analyze exit 5, and loose and over exit 7 in `outcome.json`. Those are the producer's own refusals inside the frozen fixture. They are not a new export claim. `parent_acceptance` is false and `release` is `not_claimed` in that file.

## Failed attempts kept

Smoke and the chain each exited 0 on the first launch. No `linux/smoke-failed` or `linux/chain-failed` directory was created.

`pg/actual/capture-failed-1.txt`: the first row dump rejected the table-level mapping row because `logical_column` is empty. Source rows were already the six lines.

`pg/actual/capture-failed-2.txt`: the next dump used `scope_namespace_physical` as the PostgreSQL schema. That relation does not exist. `psql` without `ON_ERROR_STOP` still exited 0 and left `target-rows.txt` empty. The retry queries `_oo` and stops on a SQL error. That retry exited 0 and the two row files match.

## Not claimed

Parent acceptance, SP05-W06, AT-FILE-05, AT-SEC-03, xlsx, HTTP authentication, download authorization, retention, publish, and release. No new unit count.
