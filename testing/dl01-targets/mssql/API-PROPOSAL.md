# MSSQL runtime contract proposal

Status: proposal only. Nothing in this file is implemented, registered, or compiled.

Base `577160c22de63adb9b38e0147f3bc2da893c4d9d` exports `PgConnector` and `PROTOCOL = "oo.source.pgsql/v1"`. There is no shared `Connector` trait and no `tiberius` dependency in the root `Cargo.toml` or `Cargo.lock`. `crates/oo-connectors/Cargo.toml` does not enable a `mssql` feature. Vendored sqlx still contains `cfg(feature = "mssql")` branches and a comment that Microsoft SQL Server is pending a full rewrite. That feature stays off. Turning it on is not this target preparation.

DBX-W01 through DBX-W05 are the contract owners for a shared source model. This batch may consume them only after the coordinator delivers their immutable SHA. Their mutable trees were not compiled and were not treated as a dependency.

## Proposed protocol

`oo.source.mssql/v1`

Dependency the contract owner would add, with defaults off:

```toml
tiberius = { version = "0.13.0", default-features = false, features = ["tds73", "rust_decimal", "rustls"] }
```

Checksum `e07324791de2bdaed058af4aa433a0b5ec5c0beac407a9797419c19805df2415`. License MIT OR Apache-2.0. Crate `rust-version` is 1.88. The repo's pinned Linux image `rust:1.94.0` (`sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`) is a newer toolchain than that floor. This task did not compile the crate, so the toolchain pin is not evidence that tiberius builds.

`tds73` is required for `datetime2`. `rust_decimal` is the decimal path. `rustls` keeps TLS off the default `native-tls` and Windows `winauth` features. `tds80` stays off until a live 2022 Developer handshake shows that TDS 8.0 strict encryption is required. `integrated-auth-gssapi` stays off for the SQL-auth fixture.

## Proposed session shape

Mirror the existing PostgreSQL session, with MSSQL facts. Do not reuse `PgSourceConfig` or fill `AccessReport.publications` / `replication_slots` with SQL Server objects.

- `MssqlEndpoint { host, port }`
- `MssqlCredentials { secret_ref }` using the existing secret-reference pattern. The value stays out of logs.
- `MssqlTls { encrypt: mandatory, trust_server_cert: false }` except a loopback Developer fixture, where trusting the container certificate is an explicit fixture flag.
- `MssqlBudget`
- `MssqlConnector::new(config).connect() -> MssqlSession`
- `MssqlSession::inspect_access` returns protocol, core contract, source id, secret ref, `SERVERPROPERTY` version, database and column collation, principal, and a write determination that does not commit a trial write.
- Capabilities for a DL01 preview are catalog and batch full only. `cdc` and change feed return unverified until DBP2.
- `dba_checklist` lists `testing/dl01-targets/mssql/dba`. The connector does not execute it.
- `MssqlSession::catalog`
- Typed reads produce `ColumnMapping` with `protocol = "oo.source.mssql/v1"` and the frozen fields `source_type`, `arrow_type`, `pg_type`, `precision`, `scale`, `timezone`, `nullable`, `type_policy_version`, plus metadata.
- `datetime2(7)` maps toward `timestamp(6)` with metadata `precision_loss`. A `datetime2(7)` primary key or cursor blocks, matching the database profile. This fixture uses `binary(4)` as the key.
- `decimal` / `numeric` stays on a decimal path. It does not pass through `f64`.
- A CI character column carries `source_collation_ci` and `trailing_space_insensitive`.
- Chunk order for the binary key is the `binary(4)` byte order. Character chunk order uses `Latin1_General_BIN2` and still has to account for ANSI trailing-space padding; exact bytes use `DATALENGTH` and `CONVERT(varbinary)`.
- Cancel is a TDS attention on the reader connection. The reader is not granted `KILL` or `ALTER ANY CONNECTION`.

## Exit until that contract exists

This preparation does not satisfy source-support acceptance. A later Rust consumer needs the contract SHA, a native `linux/amd64` SQL Server, and a compile on the pinned 1.94 image. Linux/MSRV applicability for tiberius stays pending.
