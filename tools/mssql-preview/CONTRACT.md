# mssql-preview

Customer protocol: `oo.mssql-preview.customer/v1`.

The secret is a `secret_ref` file URI or absolute path. The file must be mode `0600`. The command reads it only after preflight and does not print it. Inline password fields are refused.

`encrypt` must be `required`. `trust_server_certificate: true` is refused. With no `ca_file`, trust is the webpki root set. A `ca_file` is added as an extra CA. `trust_cert` is not called.

Driver: tiberius 0.13.0, default features off, features `tds73`, `rustls`, `rustls-webpki-roots`, `rust_decimal`. License `MIT OR Apache-2.0`. Crate sha256 `e07324791de2bdaed058af4aa433a0b5ec5c0beac407a9797419c19805df2415`. Repository `https://github.com/prisma/tiberius`. `chrono`, the `time` feature, `native-tls`, `tds80`, and integrated auth are not enabled.

`datetime2(7)` is an Int64 count of 100ns ticks since `0001-01-01`. Scale above 7, a string, or classic `datetime` is refused. PostgreSQL `timestamp(6)` is refused. `time(n)` for scale 0..=7 becomes `Time64(Nanosecond)` by multiplying by `10^(9-scale)`. `decimal`/`numeric` stay `Decimal128`. `money` is `decimal(19,4)` and `smallmoney` is `decimal(10,4)`; a float payload is refused. `nvarchar` is stored unchanged and the collation is metadata; this loader does not case-fold or sort. `binary` stays raw bytes. `uniqueidentifier` is `FixedSizeBinary(16)` in SQL Server mixed-endian wire order. Arrow's UUID type is not used.

The only SQL sent is `SERVER_SQL`, `CATALOG_SQL`, `PERMISSION_SQL`, and a generated `SELECT TOP (n)` of approved columns. `testing/dl01-targets/mssql/reader/02-negative-writes.sql` is not part of this reader. Write statements, `sp_cdc*`, and `WAITFOR` are refused before a socket. CDC, change tracking, and DBP2 are not claimed.

A deadline that is already past returns `deadline_exceeded` before a socket. After a socket exists, a command timeout sends TDS attention and then closes the connection. This reader does not send `KILL`.

`seal-request` writes `oo.pipeline.native-sealed-catalog/v1` with no `engine`, `source_engine`, `admitted`, `admission_bin`, `registry_path`, `registry_sha256`, or `approval` key, then exits 4. The pinned sealed set is oracle, clickhouse, opensearch, doris, mongodb, mongo, starrocks. `sqlserver` is not in it. This crate does not add it and does not relabel the source as another engine.

`gates` exits 5. Source identity, admission, audit, observed cancel, Builder, Pack, local consumer, and whole-source acceptance stay pending until a real Linux/amd64 SQL Server target is supplied. `encode-arrow` is a codec. `native_source` stays false. A fixture is not GOLD and is not a native read.

`scripts/run-external.sh` runs `catalog` only when `OO_MSSQL_CUSTOMER_CONFIG` and `OO_MSSQL_CATALOG_OUT` are absolute paths. It does not search for credentials and does not accept an EULA.

Exit 0: preflight or encode-arrow. Exit 2: usage or policy. Exit 3: deadline, connect, login, or live permission failure. Exit 4: native engine unrecognized. Exit 5: gates pending.

## Full snapshot and C2 implementation (native 2026-10-12)

`snapshot-full --config PRIVATE --out NEW_ABSOLUTE_DIRECTORY [--object schema.table]
[--chunk-rows 1..10000]` and `ct-pull` (same flags plus `--position APPLIED_POSITION`)
are real TDS readers. Explicit `capabilities_requested: [ChangeTracking]` is required.
A DBA must already have enabled database/table CT and snapshot isolation. The reader
never enables them. Existing customer source/secret/TLS/object/budget controls apply.
`budget.timeout_ms` is optional, 1..3600000; default 30000. One command-wide deadline
covers connection, metadata, all approved tables and full chunk enumeration.

The reader observes actual primary keys, rejects missing/nullable keys and unsupported
column types, starts one SNAPSHOT transaction, reads current/min-valid CT anchors,
counts all rows/changes with COUNT_BIG, enforces the aggregate approved row cap,
and enumerates every ordered chunk. Text keys order under BIN2; original native
collation is used in CT joins and preserved in metadata. Each original binary PK
is retained. CT delete rows have typed key Arrow distinct from live full-row Arrow.
A single valid upsert IPC stream contains all recordbatches, including a typed empty
batch for idle/delete-only captures. Independent expected data remains separate.

CT position includes actual database GUID/recovery fork, object identity, complete
column type/width/collation schema hash, primary keys and version. Expired/future
positions, restore identity, table/schema/key changes, malformed operations, missing
live-row values, count mismatch or timeout fail closed. Metadata is checked again
before commit. Output uses a private staging directory, removes partial files on
failure, and publishes only after successful source read transaction commit. This is
**a candidate position**, not target acknowledgement. No rowversion cursor is used;
MIN_ACTIVE_ROWVERSION cursor claims and C1/DBP2 remain outside this implemented C2.

`audit-export` uses a separately approved audit observer configuration and an
explicit existing audit file glob to SELECT the original reader/application's SQL
Server Audit records. It never installs audit or expands grants. Native producer
classification rejects empty/cross-reader/unknown/failed-write audit records. Raw
SQL audit remains private. The new source producer reads actual worker `native-state`,
uses only succeeded-CAS CT position for resume, runs the real reader and observer,
then calls a hash-pinned shared `seal-native`. It supplies the resulting real receipt
and registration to `oo.pipeline.native-update/v1`; it never imports, claims, creates
leases or acknowledges its own position. Failed raw phase artifacts are retained.

Original SQL Server acceptance, live cancellation/audit, CT consistency/recovery,
Self-build/Pack/Gateway/Agent and native target proof remain **pending** without the
approved native amd64 SQL2022 endpoint/EULA/config. Local decoder/parser tests do not
establish source correctness. Legacy `seal-request` and `gates` still refuse acceptance;
the new native producer delegates admission to the current pinned shared service.
