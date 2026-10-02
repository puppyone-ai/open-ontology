# oo-connectors

Read-only PostgreSQL source inspection and catalog. Protocol `oo.source.pgsql/v1`.

The frozen method and field contract is [contracts/source/pgsql/README.md](../../contracts/source/pgsql/README.md). The empty catalog fixture is [contracts/source/pgsql/empty-catalog.json](../../contracts/source/pgsql/empty-catalog.json).

## API

`PgConnector::connect`, `PgSession::inspect_access`, and `PgSession::catalog` are the public methods. The caller resolves a `secret://` reference and passes the password in memory. `Debug` redacts the password and the CA PEM. `SourceError` displays a kind, a static fact, and an optional SQLSTATE.

`inspect_access` returns an `AccessReport`. `registration_allowed` is false when block reasons are present. `catalog` returns the selected tables, or a `SourceError` when the server version is untested, a selected table is missing, or an essential catalog fact is unreadable. A write-capable source can still produce a catalog snapshot. The next import step has to read `registration_allowed` itself.

Capabilities `scan`, `bounded_read`, `cdc`, `type_conversion`, and `registry_api` are unsupported. `source_ddl` and `source_write` are forbidden. This crate does not create publications, replication slots, signaling tables, heartbeats, or other source objects, and it does not write source data. The DBA checklist is text.

## TLS and versions

Connections use `sslmode=verify-full` and trust only the caller CA. Hostname verification stays on. `TlsReport.trust` is `caller-ca-only`. `negotiated_version` is the `pg_stat_ssl` version for that backend. The driver builds `PgConnectOptions` with `from_explicit` and does not copy `PGOPTIONS`, `PGSSLCERT`, `PGSSLKEY`, or other libpq environment values, and it does not change the process environment. A `session_user` / `current_user` mismatch or an unexpected startup parameter blocks registration. The declared server major floor is 13. The only tested server version is PostgreSQL 17.10. An untested 13+ server blocks registration and `catalog` does not run.

## Supply chain

Direct driver dependencies are pinned in this crate's manifest. sqlx 0.9.0 has no public feature for the ring provider with an empty root store, and `PgConnectOptions::new` copies libpq environment. The workspace patches official `sqlx-core` 0.9.0 and `sqlx-postgres` 0.9.0 from `vendor/` with the hunks in `vendor/PATCH.md`. This crate enables `_tls-rustls-ring-no-roots` and does not enable webpki or native roots. The transitive delta is in [supply-chain/pg-source-dependency-delta.json](../../supply-chain/pg-source-dependency-delta.json). The sqlx 0.9.0 facade enables `sqlx-core`'s `migrate` feature unconditionally; this crate does not call migrate.

## Tests

The synthetic server is [testing/product/pg-source](../../testing/product/pg-source). The live test reads `OO_PG_FIXTURE` and fails if that pointer is absent. Focused Linux check, from a Rust 1.94.0 environment:

```text
cargo fetch --locked
cargo test -p oo-connectors --locked --offline
cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings
```

Workspace `rust-toolchain.toml` may name a newer compiler. The acceptance run sets `RUSTUP_TOOLCHAIN=1.94.0`.
