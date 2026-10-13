# Proposed protocol `oo.source.mariadb/v1`

Status: proposal only. Base `577160c22de63adb9b38e0147f3bc2da893c4d9d` exposes `oo.source.pgsql/v1` and has no MariaDB connector. This preparation does not add a crate, feature, or driver dependency. Downstream preview waits for a common pinned interface. Nothing here is source acceptance.

## Identity

- Product: MariaDB 11.4.13, self-managed, official image digest `sha256:0130d92c05fbf2d82adc2b86de742eaede65b2596c65d91786f8e03cd19e6a39`, platform `linux/arm64/v8`.
- Protocol string: `oo.source.mariadb/v1`.
- Native client observed here: `mariadb` 11.4.13, client 15.2, package `mariadb-client` `1:11.4.13+maria~ubu2404`.
- Server binary observed here: `mariadbd` 11.4.13-MariaDB-ubu2404.
- A later Rust driver is a separate consumer. It is not the PostgreSQL sqlx driver and it is not selected by a MySQL matrix family.

## Catalog and values the consumer must preserve

- `check_access` reports the reader grant and does not send DML or DDL. The fixture reader holds `SELECT` on `oo_synth.*` only. `SELECT` on `mysql.global_priv` returns error 1142.
- JSON columns are `DATA_TYPE=longtext` and `COLUMN_TYPE=longtext`, with a `JSON_VALID` check. JSON null and SQL NULL stay distinct.
- `BIGINT UNSIGNED` is exact decimal text. The fixture includes `18446744073709551615`. It does not fit in i64.
- `DECIMAL(38,10)` and `DECIMAL(65,30)` are exact text, not f64.
- Temporal values are `DATETIME(6)`, `DATE`, `TIME(6)`, and nullable `TIMESTAMP(6)`, rendered as ISO text. The session time zone is `+00:00`. The fixture timestamp boundary is `2038-01-19 03:14:07.000000`.
- `VARBINARY` keys order as raw bytes. On this server, empty, `0x00`, and `0x0000` are distinct primary keys.
- `read_chunk` is not implemented in this preparation.
- Cancel is `KILL QUERY` from an identity that holds `CONNECTION ADMIN` and `PROCESS`. The observed reader result is error 1317. The client keeps comments only when `--comments` is set.
- Statement proof reads `mysql.general_log` for `oo_reader` after `log_output=TABLE`. The reader log in the passing run had `write=0`.
- GTID domain is a server variable (`@@gtid_domain_id`, value 0 here). `@@gtid_current_pos` is empty while `--skip-log-bin` is set. This fixture does not claim CDC.
- The image default sql_mode accepts `0000-00-00`. A consumer that rejects zero dates needs `NO_ZERO_DATE` in strict mode. This fixture sets that mode and observes error 1292.

## Explicitly absent

No implementation of this protocol is in the base commit or in this preparation. No common worker SHA was consumed. Linux/MSRV compilation stays pending until a Rust consumer builds against an immutable source SHA.
