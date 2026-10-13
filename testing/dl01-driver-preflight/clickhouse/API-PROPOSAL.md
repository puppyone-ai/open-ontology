# ClickHouse native reader proposal

Status: proposal only. This preflight does not implement it in `oo-connectors`.

Base `577160c22de63adb9b38e0147f3bc2da893c4d9d` exports `PgConnector` and `PROTOCOL = "oo.source.pgsql/v1"`. `SourceCatalog` is the PostgreSQL document. There is no ClickHouse producer and no `clickhouse` crate in that lock. This gate does not emit `oo.source.pgsql/v1` and does not stub a producer.

The HTTP reader here is reqwest `0.13.5` and Arrow `60.0.0` from that lock, because those crates are already pinned. The preparation note that names crate `clickhouse` `0.15.2` stays a contract-owner choice. It is not a dependency of this preflight.

## Proposed protocol

`oo.source.clickhouse/v1`

Fields this preflight can actually fill from `system.tables`, `system.columns`, and `system.parts`:

- `database`, `name`, `engine`, `engine_full`
- `partition_key`, `sorting_key`, `primary_key`
- `primary_key_enforced` false when a non-FINAL read shows two versions of one `reading_id`
- column `name`, native `type`, `position`, `default_kind`, `default_expression`
- `metadata_modification_time` when `system.tables` returns it
- per-partition `partition`, `rows`, `active_parts`
- `consistency` S0, `capture` C4, `token_order` PerPartition, reader write class R0
- no CDC and no multi-statement transaction

Not proposed as success: a forged `pg_catalog_hash`, `POSITION_EXPIRED`, or a PostgreSQL catalog document with ClickHouse types pasted in.

## Cancel

Client close of the HTTP socket is the native cancel, with `cancel_http_readonly_queries_on_client_close` and the query id bound to the reader user. `KILL QUERY` from the reader is denied. A DBA `KILL` is not this driver's cancel.

## Still pending

PostgreSQL copy, Builder, Pack, the shared model, matrix promotion, and full source acceptance stay pending until a contract owner accepts this shape. Author completion of the preflight is not that acceptance.
