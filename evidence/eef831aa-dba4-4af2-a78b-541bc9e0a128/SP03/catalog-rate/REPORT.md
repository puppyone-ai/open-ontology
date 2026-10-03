# REPORT — bounded catalog reads

Session `eef831aa-dba4-4af2-a78b-541bc9e0a128`. Protocol `oo.source.pgsql.snapshot/v1` and catalog `oo.source.pgsql/v1`.

This is an implementation record. It is not an R1 acceptance, a supply acceptance, or a customer release. The 87-item goal is unchanged. Functional repair and the schema guard go to independent review after this delivery.

## Fixed commit

| | |
| --- | --- |
| Commit | `ccd93e1adc34b24ab998d90972549f8cffb59f0d` |
| Tree | `ef43877346afadcdccaf26fe6982c6b4e8b65902` |
| Parent | `182702e6e67d16de10f280eaf422fce4e2f5a5f2` |
| Subject | catalog: bound each source read and settle unused allowance |

Card baseline recorded before this work: `81a0f8f247592f5f0d484f0f549e522405e7c560`, tree `5fe33ad3d14892c1b1af0caa3d04b0989c439486`, parent `2254eab39c14a1902e65e72eb3674debc1c54f03`. Schema implementation `2254eab3` is development input. It is not accepted R1. Driver `ff783ac9` stays limited accepted. The parent above is the branch tip at commit time, including the parallel portal commit. This commit does not modify portal, vendor, or snapshot test files.

Owned paths in the commit:

- `crates/oo-connectors/src/session.rs`
- `crates/oo-connectors/src/sql.rs`
- `crates/oo-connectors/src/catalog.rs`
- `crates/oo-connectors/src/access.rs`
- `crates/oo-connectors/src/types/read.rs`
- `crates/oo-connectors/README.md`
- `crates/oo-connectors/tests/catalog_rate_live.rs`
- `contracts/source/pgsql/snapshot-owner-v1.md`

`ident.rs` was not edited. `PG17_RESERVED` was already public. Keyword admission uses that embedded set plus one.

## What the reads do

Each production `fetch_all` takes a row bound before the round trip. The bound is the caller's armed value, or 1600 when the caller did not arm. The ledger reserves that bound, the statement carries the same bound as a static `LIMIT` or an existing bind, and settlement returns unused allowance only while that one-second window is still the active entry. A failed, timed-out, or dropped read keeps the reservation. A refund does not cross into a later window, and evicting other sources does not refill an exhausted debit.

An exact natural bound may come back full. Selected tables are one row per name. Columns use 1600 per table. A primary key uses at most 32 columns, and a 32-column key is complete. A result that fills a non-natural limit is `catalog_truncated`, not a complete catalog. Keyword equality at the embedded reserved-word count plus one is `identifier` / `keyword_list`. The typed reader's direct query uses the same admission. `max_rows` still rejects a larger accepted result. `max_rows_per_sec` remains 50000. The connection cap remains 4 across 64 tracked sources.

## Commands

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform `linux/arm64`. Toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `cargo 1.94.0 (85eff7c80 2026-01-15)`. Flags `--locked --offline`. `CARGO_HOME` and `CARGO_TARGET_DIR` were the UUID volumes. No privileged mode, host network, socket mount, or shared cargo cache.

PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `server_version` from init: `17.10 (Debian 17.10-1.pgdg12+1)`. Database `snapdb`. Alias `a128pg`. Credentials stayed under `/tmp/oo-a128-eef831aa-dba4-4af2-a78b-541bc9e0a128/secrets` and were not committed.

Pre-edit smoke, container `eef831aa-dba4-4af2-a78b-541bc9e0a128-rust-smoke`, `smoke_start=2026-10-03T11:16:07Z`, `smoke_end=2026-10-03T11:16:50Z`, capacity `global=0`:

```
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --lib -- --test-threads=1
cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
```

`lib.exit=0`, `clippy.exit=0`, `docker-run.exit=0`. `snapshot_live` was not run. The pre-edit red remains a125 `live/out` exit 101.

Final verify, container `eef831aa-dba4-4af2-a78b-541bc9e0a128-rust-verify`, `verify_start=2026-10-03T11:59:02Z`, `verify_end=2026-10-03T11:59:22Z`, capacity `host_cargo=0`, `rust_builders=0`, `global=0`. The snap fixture was restored with `prereq/reinit.sh` before this run. The launcher rotated the server log once, before either test.

```
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --lib -- --test-threads=1
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test catalog_rate_live -- --test-threads=1 --nocapture
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test snapshot_live -- --test-threads=1 --nocapture
cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
```

| Phase | Exit |
| --- | --- |
| lib (55 tests) | 0 |
| catalog_rate_live | 0 |
| snapshot_live | 0 |
| clippy `-D warnings` | 0 |
| container | 0 |

`catalog_rate` printed `big_elapsed=186.976333ms` for four `max_rows=50000` catalogs, and `burn_elapsed=648.401458ms` for the locked-read reservation and the following denial. Reader statements 179, reader writes 0, admin statements 15, admin writes 8. Unmodified `snapshot_live` printed `catalog_schema_hash=1ff301767c85e585b52df42ae0ab47d1cbfe6c15205b6a0fcef98605badd8236`, the same `same_cut_hash`, `schema_inflight=schema_drift`, `schema_new_generation=catalog_identity`, `privilege_inflight=insufficient_privilege`, `privilege_new_generation=select_not_granted`, `sql_audit oo_statements=843 writer_writes=3`, and `live_complete`. Secret scan of the server log was 0. The container was not privileged and used network `eef831aa-dba4-4af2-a78b-541bc9e0a128-net`.

The shared target volume still held smoke phase files. `docker cp` of `/target/phases` therefore also copied `fetch.exit=0` and `live-fixture.status=not-run` into `verify/out/phases`. Those two names are not verify phases. `verify/out/console.log` is the verify record.

## SQL audit

`catalog_rate_live` counted `oo_snap_reader` / `oo-pg-source` statements and found no `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `CREATE`, `ALTER`, `DROP`, `GRANT`, `REVOKE`, or `COMMIT`. Admin `postgres` statements and writer statements were counted separately. `snapshot_live` then audited the same log with its own broader word list, including `COMMIT`, and passed with `writer_writes=3`.
