# Handoff to DL01-W1-06

Precheck result: `route_blocked_before_greeting`. Reuse the private target file and the existing `mysql_async` 0.36.1 call shape. This packet does not admit a reader and does not close the MySQL source card.

## Private config

Open `/tmp/oo-customer-mysql-20261012/targets.json` only. Directory mode 0700. It has two targets, each with `id`, `host`, `port`, and `database`, plus `username`, `credential_file`, and `initial_read_limits`. `password_in_this_file` is false. Read the password from the `credential_file` path. That file is mode 0600 and holds `username` and `password`. Keep the password inside the driver process or a mode 0600 MySQL defaults file. Do not pass it on argv, in a URL, in Git, in a prompt, or in a log.

Authorized limits in that file: connect 10 s, statement 5000 ms, 5 rows, 1 table, 1 connection per target.

The probe tree that already compiled this recipe is `/tmp/oo-customer-mysql-readonly-20261012/probe`. It is temporary host code, not a product change. Binary sha256 `4cf37f0385230593cca1081a2922950c803d0ab99b7e3f4a034ffcaf4103f8f6`.

## Call recipe

Pin, matching `tools/mysql-preview/Cargo.toml`:

```toml
mysql_async = { version = "=0.36.1", default-features = false, features = ["minimal", "rustls-tls", "ring"] }
```

Do not run `mysql-preview` `customer` or `run`. That path is a Postgres import and requires a CA file.

Host process, one `Conn` per target, sequential:

```text
OptsBuilder::default()
    .ip_or_hostname(host)
    .tcp_port(port)
    .user(Some(user))
    .pass(Some(password))
    .db_name(Some(database))
    .prefer_socket(false)
timeout(Duration::from_millis(10_000), Conn::new(opts))
```

This precheck left `ssl_opts` unset because the target file has no instance CA. It did not set `danger_accept_invalid_certs`. Product `connect_parts` at `tools/mysql-preview/src/customer.rs:616` is the VERIFY_CA shape once the instance PEM is supplied: root cert from that file, `danger_accept_invalid_certs(false)`, `danger_skip_domain_validation(false)`, `disable_built_in_roots(true)`. Skipping certificate checks on an intermediate 198.18.0.0/15 path does not prove server identity.

Measured stop: both `Conn::new` calls returned `UnexpectedEof` at 3007 ms and 3004 ms. The next run needs a host route that returns a MySQL greeting. Docker Desktop 29.4.1 `--network host` on Darwin is not that route. A container network failure must not be recorded as the data source being unavailable.

## After a greeting exists

Session settings only. Do not install schema on the source.

```sql
SET SESSION max_execution_time = 5000;
SET SESSION transaction_read_only = 1;
START TRANSACTION READ ONLY;
```

Identity, for a private file. Git may keep the version string, charset and time zone summary, and match booleans for database and account:

```sql
SELECT VERSION(), DATABASE(), CURRENT_USER(), @@version_comment, @@version_compile_os, @@hostname,
       @@character_set_client, @@character_set_connection, @@character_set_results, @@character_set_server,
       @@collation_connection, @@collation_server, @@time_zone, @@system_time_zone, @@session.time_zone;
```

`SHOW GRANTS FOR CURRENT_USER()` goes to a mode 0600 file. Parse verbs. A password-hash clause stays in that file. If the grants include write, admin, replication, or unknown verbs, record non-R0. Do not label the account read-only. Do not register it as the platform admitted reader. This observer's own statements can stay R0 only when they are read-only queries and create no source objects.

One base table:

```sql
SELECT TABLE_NAME, ENGINE, TABLE_COLLATION, TABLE_ROWS, DATA_LENGTH
FROM information_schema.TABLES
WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE'
ORDER BY TABLE_ROWS IS NULL, TABLE_ROWS, TABLE_NAME
LIMIT 1;
```

`TABLE_ROWS` is an estimate. Do not run `COUNT(*)` on the base table. Column metadata, at most 64:

```sql
SELECT COLUMN_NAME, ORDINAL_POSITION, DATA_TYPE, COLUMN_TYPE, IS_NULLABLE,
       CHARACTER_MAXIMUM_LENGTH, NUMERIC_PRECISION, NUMERIC_SCALE, CHARACTER_SET_NAME,
       COLLATION_NAME, COLUMN_KEY
FROM information_schema.COLUMNS
WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?
ORDER BY ORDINAL_POSITION
LIMIT 64;
```

Keep real table names, column names, and ENUM/SET member lists in the private archive. Git gets aliases, data-type histogram, and row, column, and cell counts. Then `SELECT` the chosen columns `LIMIT 5` inside the read-only transaction, statement cap 5000 ms. Store raw row bytes in a mode 0600 file. Publish row hashes and counts. `ROLLBACK` and drop the connection.

Optional existing evidence: `performance_schema.events_statements_history` for this `CONNECTION_ID` only. If that read is denied, source audit stays unobservable.

## What 06 still owns

Updates, self-build, YAML ontology, Knowledge Pack, Gateway, and Agent on the original MySQL source card. This precheck supplies the private target config and the reader call above. It supplies no server version, no grant classification, and no sample rows, because the greeting never arrived.

Minimal missing condition now: a host path on which port 3306 returns a MySQL server greeting. Later missing condition for VERIFY_CA: the instance CA PEM, which is not in `targets.json`.

Model and SQL Server remain pending. Do not add a source to the denominator from this packet.
