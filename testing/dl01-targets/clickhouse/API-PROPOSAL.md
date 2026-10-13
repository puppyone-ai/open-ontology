# ClickHouse runtime contract proposal

Status: proposal only. Nothing in this file is implemented, registered, or compiled.

Base `577160c22de63adb9b38e0147f3bc2da893c4d9d` exports `PgConnector` (`crates/oo-connectors/src/session.rs`) and `PROTOCOL = "oo.source.pgsql/v1"` (`crates/oo-connectors/src/lib.rs`). There is no ClickHouse module and no `clickhouse` crate in the root lockfile. Do not reuse `PgSourceConfig`, `SourceCatalog`, or `oo.source.pgsql/v1` as a stand-in. DBX-W01 through DBX-W05 remain the contract owners. This preparation does not consume their mutable trees.

## Proposed protocol

`oo.source.clickhouse/v1`

Dependency the contract owner would add, with defaults off:

```toml
clickhouse = { version = "0.15.2", default-features = false, features = ["lz4"] }
```

Checksum `7654154fde4d97ec1321bfe4c3965570be6fcd09e091da8afc3166ab5ee27be2`. License MIT OR Apache-2.0. Crate `rust-version` is 1.89.0. The HTTP interface is the P1 path. Native port 9000 is out of this fixture.

## Proposed session shape

- `ClickHouseEndpoint { host, http_port }`
- `ClickHouseCredentials { secret_ref }` using the existing secret-reference pattern. The value stays out of URLs, argv, and logs. This fixture sent `X-ClickHouse-User` and `X-ClickHouse-Key` from memory.
- `ClickHouseBudget { max_execution_time, max_result_rows, max_result_bytes, function_sleep_max_microseconds_per_block }`
- `ClickHouseConnector::connect() -> ClickHouseSession`
- `catalog` returns database, table, engine, `engine_full`, partition key, sorting key, and primary key as physical hints. Those keys are not an enforced transactional primary key. Column rows keep the native type string, position, `default_kind`, and `default_expression`.
- `read_partition(table, partition, final)` is the product read. `final` is true for a consistent ReplacingMergeTree read. A false value is a diagnostic that still returns superseded parts.
- `cancel(query_id)` is a control connection: observe `system.processes`, then `KILL QUERY WHERE query_id = ... SYNC`. The reader account does not receive `KILL`.
- The reader audit reads `system.query_log` after `SYSTEM FLUSH LOGS` on the control connection. A successful `query_kind` outside `Select` fails the audit. Fixture DDL stays on the DBA user and is not mixed into that filter.

Typed values from this fixture, independent of a later consumer:

- `UInt64` stays a decimal integer string. `18446744073709551615` must not pass through `f64`.
- `Decimal(76, 10)` is Decimal256. The example `1234567890123456789012345678901234567890.1234567890` does not fit Decimal128. Render with an explicit scale (`toDecimalString` on this server). `toString` dropped trailing zeros in attempt-04.
- `Nullable(T)` stays nullable. `LowCardinality(T)` is the inner type at the logical layer.
- `DateTime64(9, 'UTC')` is `precision_loss` against PostgreSQL microseconds. Block the column when it is a key or cursor. `reading_id` is the key in this fixture.
- `Array(Int64)` and `Array(Nullable(String))` need a consumer mapping that does not exist yet.
- A non-null default makes an omitted value indistinguishable from an explicit default. Rows 102 and 103 both store `qty = 0` and `label = ''`. The caveat is `default_for_missing`.
- Product reads of `ReplacingMergeTree` use `FINAL`. Without `FINAL`, reading 101 kept versions 1 and 2 while merges were stopped. Consistency is S0. Capture is C4 through `system.parts` (partition, rows, active parts). Token order is per partition. This prep does not claim a total position or `POSITION_EXPIRED`.
- Lightweight `DELETE` is asynchronous. Record `system.mutations.is_done` and the visible ids. Do not invent a lag that the server already applied.
- Reader write class is R0. The zero-write witness is the DBA checksum of `oo_dl01.readings` plus the reader `query_log`, not a statement from the reader.
- There is no multi-statement transaction and no CDC.

## Exit until that contract exists

Arrow IPC, PostgreSQL `COPY`, Builder, and Pack wait on a committed interface. This preparation does not emit them. Linux/MSRV applicability of crate `clickhouse` 0.15.2 stays pending until a Rust consumer compiles it on the pinned 1.94 image. Author completion of the fixture is provisional until a separate fixed review and coordinator acceptance.
