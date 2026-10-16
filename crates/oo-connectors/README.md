# oo-connectors

Read-only PostgreSQL source inspection and catalog (`oo.source.pgsql/v1`), with an optional MySQL C1 batch reader.

The frozen method and field contract is [contracts/source/pgsql/README.md](../../contracts/source/pgsql/README.md). The empty catalog fixture is [contracts/source/pgsql/empty-catalog.json](../../contracts/source/pgsql/empty-catalog.json).

## API

`PgConnector::connect`, `PgSession::inspect_access`, and `PgSession::catalog` are the public inspection methods. The caller resolves a `secret://` reference and passes the password in memory. `Debug` redacts the password and the CA PEM. `SourceError` displays a kind, a static fact, and an optional SQLSTATE.

`SnapshotOwner` is a separate protocol, `oo.source.pgsql.snapshot/v1`. The contract is [contracts/source/pgsql/snapshot-owner-v1.md](../../contracts/source/pgsql/snapshot-owner-v1.md). `open` reserves a generation and holds one data connection plus one current-catalog connection, and one `REPEATABLE READ READ ONLY` transaction on the data connection. `PgSession::catalog` is not that transaction: it still begins and rolls back on its own. `read_probe` is one primary-key lookup (`LIMIT 2`) decoded with the existing type converter. It is not a batch stream and not the bounded reader. `finish` returns `SnapshotFinished` as lifecycle close only, after `ROLLBACK` and connection close both succeed. That value is not admission. `finish_complete` returns `RunComplete` as full-selection admission only after this owner delivered `StreamItem::Exhausted` for every selected table and the existing release checks passed. `abort` returns `SnapshotAborted` on that same cleanup. Drop closes the backend when it can and does not return either value. `scan` and `bounded_read` are supported for the PostgreSQL full-mode snapshot, the bounded pull, and `RunComplete` admission. The reason is `PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission`. R2 (bounded Arrow batch stream and budgets), R3 (key planning, cancellation, and whole-round failure), and R4 (full-selection admission on that same path) have that limited acceptance. The pull is [contracts/source/pgsql/bounded-stream-v1.md](../../contracts/source/pgsql/bounded-stream-v1.md). Key chunks are [contracts/source/pgsql/reader-keys-v1.md](../../contracts/source/pgsql/reader-keys-v1.md) and are not full-run admission. Cancel and unpublished-run failure are [contracts/source/pgsql/reader-lifecycle-v1.md](../../contracts/source/pgsql/reader-lifecycle-v1.md). Loss of the snapshot restarts the whole unpublished run. A new generation does not mix rows from the old cut. This acceptance is not full types, CDC, incremental mode, a registry API, source DDL, source write, a customer proof, or a release. No production target loader admits `RunComplete`. The reader token is the prerequisite only.

`inspect_access` returns an `AccessReport`. `registration_allowed` is false when block reasons are present. `catalog` returns the selected tables, or a `SourceError` when the server version is untested, a selected table is missing, or an essential catalog fact is unreadable. A write-capable source can still produce a catalog snapshot. The next import step has to read `registration_allowed` itself.

Each catalog, access, and typed read reserves that statement's row bound before the round trip and puts the same bound in a static `LIMIT`. The reservation settles to the rows actually returned, in the same one-second window. Unused allowance is released only in that window. A failed or timed-out read keeps the reservation. An exact natural bound, such as the selected-table count, the 1600-column ceiling, or a primary key of at most 32 columns, may come back full. A result that fills a non-natural limit is `catalog_truncated`. Keyword admission reserves the embedded reserved-word count plus one. `max_rows` still rejects a result larger than the configured budget. `max_rows_per_sec` remains 50000 for the source, and the connection cap remains 4 across 64 tracked sources.

`connector_capabilities` reports `scan` and `bounded_read` supported, with the reason above. `cdc`, `type_conversion`, and `registry_api` stay unsupported (`not_in_this_slice`). `source_ddl` and `source_write` stay forbidden. Type capability `bounded_reader` uses the same reason. Per-column `type_support.converted` stays false, and catalog `type_conversion` stays unsupported. Those decode entries are not a whole-type certificate. This crate does not create publications, replication slots, signaling tables, heartbeats, or other source objects, and it does not write source data. The DBA checklist is text.

## TLS and versions

Connections use `sslmode=verify-full` and trust only the caller CA. Hostname verification stays on. `TlsReport.trust` is `caller-ca-only`. `server_observation` is `observed` and `negotiated_version` is the `pg_stat_ssl` version only when that view returns a row for the backend. A missing row is `unknown`: not plaintext, and not a negotiated version. The driver requirement is not weakened by that null. `ssl = false` still fails as `tls_not_negotiated`. The driver builds `PgConnectOptions` with `from_explicit` and does not copy `PGOPTIONS`, `PGSSLCERT`, `PGSSLKEY`, or other libpq environment values, and it does not change the process environment. A `session_user` / `current_user` mismatch blocks registration, and `catalog` returns `session_user_mismatch` without a snapshot. A server `ALTER ROLE ... SET role` is also `startup_option`, because `current_setting('role')` is not the default `none` and `role` is outside the allow-list. The connector does not send `SET ROLE`, `RESET ROLE`, or `SET SESSION AUTHORIZATION`. The declared server major floor is 13. The only tested server version is PostgreSQL 17.10. An untested 13+ server blocks registration and `catalog` does not run.

## Supply chain

Direct driver dependencies are pinned in this crate's manifest. sqlx 0.9.0 has no public feature for the ring provider with an empty root store, and `PgConnectOptions::new` copies libpq environment. The workspace patches official `sqlx-core` 0.9.0 and `sqlx-postgres` 0.9.0 from `vendor/` with the hunks in `vendor/PATCH.md`. This crate enables `_tls-rustls-ring-no-roots` and does not enable webpki or native roots. The transitive delta is in [supply-chain/pg-source-dependency-delta.json](../../supply-chain/pg-source-dependency-delta.json). The sqlx 0.9.0 facade enables `sqlx-core`'s `migrate` feature unconditionally; this crate does not call migrate.

## Tests

The synthetic server is [testing/product/pg-source](../../testing/product/pg-source). The live test reads `OO_PG_FIXTURE` and fails if that pointer is absent. `tests/snapshot_live.rs` reads `OO_PG_SNAPSHOT_FIXTURE` and fails if that pointer is absent. It is the R1 snapshot-owner fixture, not the package catalog fixture. Focused Linux check, from a Rust 1.94.0 environment:

```text
cargo fetch --locked
cargo test -p oo-connectors --locked --offline
cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings
```

Workspace `rust-toolchain.toml` may name a newer compiler. The acceptance run sets `RUSTUP_TOOLCHAIN=1.94.0`.

## MySQL (optional)

Enable `features = ["mysql"]` to expose `oo_connectors::mysql`. The default PostgreSQL build does not enable `mysql_async`. Source connection, version checks, catalog mapping and value decoding live in `src/mysql/`; this module has no pipeline or target credentials.

The caller resolves `MysqlConfig.secret_ref`, connects with the caller CA and hostname verification, then calls `inspect` (SELECT-only access) or `inspect_replication` (the C1 SELECT/replication account). `ReadConnection` adopts that checked connection and the caller-owned read-only snapshot. `TableReader` binds one selected table to `RunBinding` and pulls integer-primary-key keyset pages. Each `read_chunk` returns a `TypedBatch`, source rows for the C1 frame adapter, rejects and a statement note. The C1 consumer opens its durable target stage before starting the source snapshot; the connector does not create that stage or commit target data.

Limits are at most 10,000 rows per query, 1,000,000 rows per table, 32 MiB per-table raw-byte budget and 16 MiB per batch (including Arrow allocation), a 300 s table deadline and 30 s statement timeout; consumers may choose smaller bounds. Oversized cells and exceeded budgets fail closed. `Cancel` and failed pulls consume the owned connection and kill its source backend through a separate connection using the same source account. Successful callers recover the connection to commit or roll back their snapshot. Dropping a future is not a cleanup receipt.

MySQL 5.7 and 8.x status syntax and the existing exact decimal, unsigned integer, binary, JSON, NULL, zero-date and signed TIME conversions are shared with the actual C1 baseline and reconcile consumers. This is a batch API, not a new CDC protocol or a whole-source admission token. The ignored `mysql_live` test takes private `OO_MYSQL_MAINLINE_FIXTURES` and a new `OO_MYSQL_MAINLINE_OUT` directory; it checks both real versions against immutable full Arrow references and checks TLS, cancellation and budget cleanup.
