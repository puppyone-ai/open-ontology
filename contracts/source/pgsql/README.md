# PostgreSQL source catalog protocol `oo.source.pgsql/v1`

This slice freezes the read-only inspection API implemented by `oo-connectors`. It does not implement scan, bounded read, CDC, type conversion, or a registry API. SP03-W01 is not complete. PostgreSQL 13 is the declared major floor. The only tested server version is `17.10`.

## Methods

| Method | Input | Output |
|---|---|---|
| `PgConnector::new` | `PgSourceConfig` | connector |
| `PgConnector::connect` |  | `PgSession` or `SourceError` |
| `PgSession::inspect_access` |  | `AccessReport` |
| `PgSession::catalog` |  | `SourceCatalog` |

`PgSourceConfig::try_new` takes `SourceId`, `PgEndpoint`, `PgCredentials`, `SecretRef`, `PgTlsConfig`, `PgBudget`, and `TableSelection`. The caller resolves `secret://` and passes the password in memory. Config `Debug` redacts the password and the CA PEM. `SourceError` `Display` is a kind, a static fact, and an optional SQLSTATE.

## TLS

The driver sets `sslmode=verify-full` and requires a caller CA PEM. Trust is `caller-ca-only`: the patched sqlx-core ring provider starts from an empty root store and adds only that PEM. Mozilla webpki roots and system roots are not installed. `negotiated_version` reports `pg_stat_ssl.version`. Hostname verification uses the endpoint host. Connection options come from the validated config via `PgConnectOptions::from_explicit`. Ambient `PGOPTIONS`, `PGSSLCERT`, `PGSSLKEY`, `PGUSER`, `PGPASSWORD`, `PGDATABASE`, `PGAPPNAME`, `PGSSLMODE`, and `PGSSLROOTCERT` are not copied and the process environment is not modified. Paths that start with `/`, DSNs, and empty passwords are rejected before connect.

## Access report

`registration_allowed` is false when `block_reasons` is non-empty. Block codes:

| Code | Meaning |
|---|---|
| `version_unknown` | Version query could not be classified |
| `version_below_minimum` | Major below 13 |
| `version_untested` | Major is at least 13 and the version is not `17.10` |
| `essential_fact_unknown` | A required fact was not readable. This is not absence |
| `write_capable` | Reachable business write: table or column `INSERT`/`UPDATE`/`DELETE`/`TRUNCATE` on any user table, ownership, schema `CREATE`, current-database `CREATE`, direct, inherited, or `SET ROLE` |
| `privileged_role` | Assumable `rolsuper`, `rolbypassrls`, `rolcreaterole`, or `rolcreatedb` |
| `security_definer` | A reachable non-catalog security-definer function, including operator and cast paths. Table read-only does not make this safe |
| `role_mismatch` | `session_user` and `current_user` differ. `catalog` returns `session_user_mismatch` |
| `startup_option` | A client, session, or startup parameter outside the driver allow-list was set. `catalog` returns `startup_options` |
| `selected_table_missing` | A selected table was not visible |

`essential_fact_unknown` also covers `privilege_scan_truncated` and `inaccessible_role`. The scan counts user schemas, tables, security definers, and definer operators and casts. It stops before dumping privileges when a count or a count times the assumable-role width exceeds `max_rows`. It does not read table data or function bodies. An unrelated role that this login cannot see is not a finding. `TEMP` and `CONNECT` are not business writes. `rolcreatedb` is `privileged_role`. `CREATE` on the current database is `write_capable` with path `database`.

`rolreplication` is a CDC fact. By itself it does not block registration. Publication or slot state `unknown` is not the same as `absent`. Unknown publication or slot state blocks registration only when `wal_level` is `logical`. `max_slot_wal_keep_size = -1` is a checklist warning.

`Fact.status` is `known`, `absent`, `unknown`, or `not_collected`. Write `state` is `read_only`, `write_capable`, `unknown`, or `not_collected`.

The DBA checklist is text. The connector does not create publications, slots, signaling tables, heartbeats, or other source DDL, and it does not write source data. Example passwords use the placeholder `<由 DBA 设置>`.

Capabilities `scan`, `bounded_read`, `cdc`, `type_conversion`, and `registry_api` are `not_in_this_slice`. `source_ddl` and `source_write` are `forbidden`. `cdc.implemented` is false even when prerequisites are present.

## Catalog

`catalog` returns selected tables only. Names are the original server names plus `pgident/v1` stable identifiers. Reserved words, quoted names, and names that start with `q_` are hex-encoded. Two originals that share a stable identifier fail with `identifier_collision`.

Each column keeps `attnum`, type oid, typmod, nullability, collation, identity, generated marker, and default hash. Domains keep their oid and `base_type_oid`. Enums keep their oid and labels. `numeric` with precision above 38, and unconstrained `numeric`, use reason `unbounded_or_wide_numeric`. Other unconverted types use `known_builtin_not_converted`, `domain_not_converted`, `enum_not_converted`, or `unknown_type_oid`. `type_support.converted` is false. `type_conversion` on the catalog is `unsupported`.

Comments, default expressions, policy expressions, and domain checks are `UntrustedEvidence` (`byte_len`, `sha256`). They are not executed. `Debug` of that type does not include the body.

`schema_hash` is SHA-256 of the canonical structural JSON (`pg-catalog-hash/v1`). The preimage includes names, column structure, keys, foreign keys, and hashes of defaults, policies, and domain checks. It excludes comments, privilege booleans, and `captured_at`. The empty selection hashes as `{"tables":[]}`:

```text
cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc
```

`empty-catalog.json` is that shape. A missing permission or an untested version makes `catalog` return `SourceError` rather than a partial snapshot.

## Budgets

Connect, statement, and lock timeouts are at most 120 seconds. `max_rows` is at most 100000. `max_bytes` is at most 32 MiB. The session sets `statement_timeout`, `lock_timeout`, `idle_in_transaction_session_timeout`, and `default_transaction_read_only=on`, then reads inside `REPEATABLE READ, READ ONLY` and rolls back.
