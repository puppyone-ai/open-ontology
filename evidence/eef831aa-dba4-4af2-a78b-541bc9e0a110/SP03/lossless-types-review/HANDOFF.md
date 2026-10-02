# a110 handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a110`. Independent review of fixed commit `3ca5357930a2c44c42d3061db5fabe577881269a`, tree `ec11cd3ee7948c33e6d43cd2e984e01b948f94e6`, parent `dd5ecf39604b8467a0d1f91d56d2e90473fa2c48`.

Verdict: **request_changes**.

`cargo deny --config ci/deny.toml --locked check` exits 4. `licenses FAILED`. New `tiny-keccak` 2.0.2 is `CC0-1.0` through `arrow-array`'s wasm32 `ahash` feature `compile-time-rng`. `ci/deny.toml` does not allow CC0-1.0 and sets no `[graph].targets`. The host aarch64 normal tree does not compile that crate. The same command also rejects inherited `webpki-root-certs` 1.0.9 `CDLA-Permissive-2.0` via unchanged `oo-sdk`.

The production type path on this commit is separately approved for the scope below. That approval does not clear the supply gate.

## Approved scope

- `cargo test -p oo-connectors --lib --locked --offline` exits 0. 61 passed: 45 crate tests and 16 `review_wire` tests. Linux `rustc 1.94.0` `aarch64-unknown-linux-gnu`.
- `cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings` exits 0.
- `cargo test -p oo-connectors --test independent_gold --locked --offline -- --ignored` exits 0 against PostgreSQL `17.10 (Debian 17.10-1.pgdg12+1)`, private CA, `verify-full`, reader `oo_a110_reader`.
- Live `PgSession::read_typed` returns `TypedBatch::record_batch()` from `arrow-array` 60.0.0 in the `oo-connectors` graph. Anchor `numeric(10,-2)` value 1200 is `Decimal128(10, -2)` coefficient 12. Live place wire `00010001000000000001` decodes to coefficient `10000`. Catalog scale for `numeric(10,-2)` stays 2046.
- Log audit of the retained PostgreSQL log: `reader_sql=782`, `select=738`, `start=22`, `rollback=22`, `forbidden=0`, `canary=0`, `password_hits=0`. `lab.gold` md5 stays `9ddf6520d9c8e1f78b2dbbdd1c5242cc`. Insert, update, and delete counts on `gold`, `anchor`, and `canary_sink` stay at the pre-read values. Sink count stays 0.
- `cargo audit --json` exits 0 with zero vulnerabilities.
- New crate checksums match the lock. None are yanked. Each `version.created_at` is at least 604800 seconds before `2026-10-02T13:30:04Z`.

## Unsigned

- `cargo test -p oo-connectors` exits 101. `live_source_catalog` panics because `OO_PG_FIXTURE` is absent. That is not a pass.
- Byte budget is applied after `fetch_all`. No PostgreSQL `CancelRequest` was observed.
- Parent Reader, Loader, SP03-W04, AT-SRC-03, AT-SRC-05, vertical, amd64, supply release, and product release are not signed.

## Residuals

- Evidence, including attempts 1 through 5, the cumulative PostgreSQL log, and `licenses/provenance-version.tsv`, stays. `licenses/provenance.tsv` recorded the wrong `created_at` field and is not the age proof.
- The evidence snapshot has the reviewer probe copied in (`review_wire`, `independent_gold`, `log_audit`). `Cargo.lock` sha256 remains `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f`. The product worktree was not committed.
- Author a098 historical Python and the four-reset reader log were left in place.
- Own containers, volumes, network, and `/tmp/oo-a110-*` secrets are removed after the raw files and hashes were saved. No global prune.

Details are in `review.md` and `findings.json`.
