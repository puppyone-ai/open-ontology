# a333 actual PostgreSQL decimal export chain

Session `eef831aa-dba4-4af2-a78b-541bc9e0a333`. Same development repair of FILE-D04 / E44. No new plan child. The unit-only packet and its SHA256SUMS stay where they were. This directory is the later compiled chain.

The exporter binary is the archive of `62b4afecdd377fa94310f46467c48a01e61d14db`. The query-pg binary is the archive of `21cc96241a841b47dbbcb116549030e38a643ba0`, the producer a340 marked APPROVE_LIMITED. Neither archive is the moving worktree. `tools/query-result-export` was not edited in this resume. The producer was not edited.

## What ran

Private PostgreSQL 17.10 `oo-a333-pg` `8343562e63abda6c327d12c8d13f2a406f4a8240713e027b6b1201202041a1fe`, 1 GiB, 1 CPU, no published port. The frozen `a327_live::live_numeric_proof` created the six-row `shop.entry` fixture and the compiled `query-pg` wrote `linux/chain/steps/amounts.stdout`. A second invocation of that same binary, with `max_rows` changed from 20 to 1 and the SQL left unchanged, wrote `steps/partial.stdout`. The frozen `query-result-export` then wrote three exports.

Hand gold CSV files were written in `gold/` before either container. The Rust checker `tools/chain-oracle.rs` compared those bytes with the exporter output. It does not build the CSV from the JSONL.

| Check | Result |
| --- | --- |
| Exporter smoke, Rust 1.94, 22 + 3 tests | exit 0 |
| Exporter Clippy `-D warnings` | exit 0 |
| Live numeric proof | 1 passed, 2.30s, exit 0 |
| Complete export | exit 0, `gold_match=1` |
| Truncated export | exporter exit 1, `gold_match=1` |
| Formula-header export | exit 0, `gold_match=1` |
| Producer `Cargo.lock` | unchanged `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474` |

`amounts.stdout` is 1527 bytes, SHA-256 `498ff77d32d24b18f556694e8ea73d398a5a622e79907861bf9b68a584c97b5f`. That is a different file from the a340 receipt `f2cb313b2f182b2e1ab4dd5fa6598f08e9f8e1d044977924cb51c3d493658da9`. Both summaries carry the same SQL `original_sha256` `466f22e579afe5252e78ae6cffca1da0a9c4630ba8cb63440a0fc9b7a9c6957a`. This run's `executed_sha256` is `f270caf5314b1420c30661bbadb986bd51fa0a6cfecbd75bd5a09c200ece2031`.

`psql` on `oo_a327_src.shop.entry` and on the target physical numeric columns returned the same six lines: the positive 38-digit value, the negative pair, `0.0001`, `-0.0001`, `0.0000`, and NULL. Those lines are `pg/actual/source-rows.txt` and `pg/actual/target-rows.txt`.

## Limits

This report does not edit the ledger. It does not accept SP05-W06, AT-FILE-05, AT-SEC-03, or a release. xlsx, download authorization, retention, deletion, and the reaper stay parent work. The live SELECT has no text column, so the U+FEFF text cell remains the unit gold re-run in the smoke. It is not a new query-pg text receipt. The formula check on the compiled stdout is the typed decimal minus, which is not apostrophe-prefixed, and one caller header `-wide`, which is.
