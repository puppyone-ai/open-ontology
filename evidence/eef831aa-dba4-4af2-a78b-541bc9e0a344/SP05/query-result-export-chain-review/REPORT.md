# a344 query-result export chain review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a344`. Independent review of frozen FILE-D04 / E44. No development slot. No product writes. The exporter archive is `62b4afecdd377fa94310f46467c48a01e61d14db`. The producer archive is `21cc96241a841b47dbbcb116549030e38a643ba0`. Both compiles used `git archive` of those commits. The moving worktree was not mounted.

Child verdict: **LIMITED**.

The limited acceptance is the frozen E44 export chain only. Parent SP05-W06, AT-FILE-05, AT-SEC-03, xlsx, HTTP authentication, download authorization, retention, deletion, the reaper, publish, and release stay open. This report does not accept a parent plan item and does not release.

## What was already on the record

a331, fixed `15558917e2c5be12b74dd1d64fb542b7d05a03af`, withheld E44. Its `SHA256SUMS` is 163 lines. The withhold is the exact-decimal clause. The int, bool, text, null, formula, sidecar, overwrite, and partial checks held and did not replace that clause. U+FEFF was observed unprefixed and was not the withhold.

a333's unit packet implemented kind `decimal` with coefficient and scale and one U+FEFF skip, then withheld the live chain because no compiled `query-pg` stdout existed yet. The later a333 actual-chain packet exported a private PostgreSQL stdout and matched its own gold. Its chain `SHA256SUMS` is 414 lines. This review did not rehash that tree and did not copy its stdout, CSV, or exit receipts.

The original decimal requirement stays the coefficient and scale cell. Integer text is still kind `int`.

## What this review ran

Private PostgreSQL 17.10 `oo-a344-pg` `c486e090aaa3e03945146d4edbd199a1ed24c4c294d3b03052a72c88124bfbc3`, 1 GiB, 1 CPU, no published port. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. The frozen `a327_live::live_numeric_proof` created the six-row `shop.entry` fixture. The compiled `query-pg` wrote `linux/chain/steps/amounts.stdout`. The same binary, with `max_rows` changed from 20 to 1 and the SQL left unchanged, wrote `steps/partial.stdout`. The frozen `query-result-export` wrote the complete CSV, the truncated CSV, and the formula-header CSV.

Hand gold was written in `gold/` from the frozen coefficients and the CSV contract before either container. `tools/chain-oracle.rs` compared those bytes with the exporter output. It does not build the CSV from the JSONL.

| Check | Result |
| --- | --- |
| Exporter smoke, Rust 1.94, 22 + 3 tests, FEFF unit ok | exit 0 |
| Exporter Clippy `-D warnings` | exit 0 |
| Live numeric proof | 1 passed, 2.40s, exit 0 |
| Complete export | exit 0, `gold_match=1` |
| Truncated export | exporter exit 1, `gold_match=1` |
| Formula-header export | exit 0, `gold_match=1` |
| Source rows and target physical rows | same six lines, capture exit 0 |
| Producer `Cargo.lock` | unchanged `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474` |

`amounts.stdout` is 1527 bytes, SHA-256 `20623e7dc575b618223ac383ceb1b2e9a9e3f9efc1695c58ea2373bcaff4b3eb`. That file is a different receipt from the a333 stdout `498ff77d32d24b18f556694e8ea73d398a5a622e79907861bf9b68a584c97b5f`. Both summaries carry SQL `original_sha256` `466f22e579afe5252e78ae6cffca1da0a9c4630ba8cb63440a0fc9b7a9c6957a`. This run's `executed_sha256` is `7542ccf3a4adf8ff3784b592b0ee6b2acce0e205393c1f43cf366d179c9c1f64`.

`psql` on `oo_a327_src.shop.entry` and on the target physical `numeric` columns in `_oo` returned the same six lines: the positive 38-digit value, the negative pair, `0.0001`, `-0.0001`, `0.0000`, and NULL. Those lines are `pg/actual/source-rows.txt` and `pg/actual/target-rows.txt`.

## Limits

This report does not edit the ledger. It does not accept SP05-W06, AT-FILE-05, AT-SEC-03, or a release. xlsx stays `unsupported`. `http_auth` and `authenticated` are false. Download authorization, retention, deletion, and the reaper stay parent work. The live SELECT has no text column, so the U+FEFF cell remains the original unit gold re-run in the smoke. The formula check on the compiled stdout is the typed decimal minus, which stays unprefixed, and one caller header `-wide`, which is written `'-wide`.
