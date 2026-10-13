# oo.source-admission contract

Status: frozen for this crate. This file is the public protocol. It does not change `oo-connectors`, `oo-core`, or the support matrix.

The illustrative enums in `docs/database-support/02-connector-architecture.md` §2 are represented here as closed strings. Unknown strings are blocks. They are not coerced and they are not new enum variants.

## Protocols

| Name | Value |
|---|---|
| Report | `oo.source-admission.report/v1` |
| Config | `oo.source-admission.config/v1` |
| Decision | `oo.source-admission.decision/v1` |
| Observe | `oo.source-admission.observe/v1` |
| Audit | `oo.source-admission.audit/v1` |
| Existing PostgreSQL report | `oo.source.pgsql/v1` |

JSON objects with a duplicate key are invalid input. Missing required fields and unknown fields are invalid input. A present string outside a closed set is a block, not a default.

## Closed sets

Capability: `Catalog`, `Logic`, `QueryLog`, `BatchFull`, `BatchIncremental`, `ChangeFeed`, `Cdc`, `DeleteNative`, `DeleteReconcile`, `SnapshotToken`, `ReplicaRead`, `Budget`.

CursorKind: `Timestamp`, `RowVersion`, `Sequence`, `PartitionModified`.

FeedKind: `ChangeTracking`, `ChangesClause`, `ChangeDataFeed`, `SnapshotDiff`.

CdcKind: `LogicalDecoding`, `Binlog`, `LogMiner`, `SqlServerCdcTables`, `ChangeStream`, `ExternalBuffer`.

TokenOrder: `Total`, `PerPartition`, `Opaque`, `None`.

BudgetUnit: `Rows`, `Bytes`, `Credits`, `Dbu`, `ReadUnits`, `WallClock`. The spelling is exact.

Level: `candidate`, `designed`, `experimental`, `verified`, `certified`, `excluded`. This crate does not write those grades into the matrix. Adapted PostgreSQL capabilities stay `designed`.

Consistency: `T`, `S1`, `S0`, `B`.

Capture method: `C1`, `C2`, `C3`, `C4`, `C5`.

Write class: `R0`, `R1`, `R1w`, `R2`, `R3`.

Permission: `SELECT`, `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `REFERENCES`, `TRIGGER`, `CREATE`, `USAGE`, `CONNECT`, `TEMP`, `EXECUTE`, `REPLICATION`, `BYPASSRLS`, `SUPERUSER`, `CREATEDB`, `CREATEROLE`.

Granted SQL permissions that admit: `SELECT`, `USAGE`, `CONNECT`. Any other granted SQL permission blocks as `PERMISSION_NOT_READONLY`. An empty list, an unknown name, or a status other than `known` blocks as `PERMISSION_UNKNOWN`. Product tokens do not add names to this set.

SQL compatibility: `postgres-native`, `mysql-native`, `mariadb-native`, `oracle`, `sqlserver`, `db2`, `mysql-mode`, `oracle-mode`, `postgres-mode`, `sqlserver-mode`. These names stay the SQL set. A token outside this set and outside Native report interpretation is `COMPAT_MODE_UNKNOWN`.

## Native report interpretation

`admit` is still the only admission function. `compat_recognized` and `interpret_native_grant` name the mapping. They do not admit a report and they do not bypass the budget, write-class, R1w, driver, identity, or option checks.

Product compat tokens, exact strings from the source preview protocols:

| Token | Source of the token | Read-only name | Granted name that is not read-only |
|---|---|---|---|
| `mongodb-native` | `tools/mongodb-preview` `OBSERVED_COMPAT` | `read` | MongoDB built-in roles listed below |
| `starrocks-native` | `tools/starrocks-preview` | `SELECT`, `USAGE`, `CONNECT` | the other SQL permission names |
| `doris` | `tools/doris-preview` `compat_mode` | `SELECT`, `USAGE`, `CONNECT` | the other SQL permission names |
| `clickhouse-native` | `tools/clickhouse-preview` `COMPAT_MODE` | `SELECT` with an exact database or table scope, plus `native.clickhouse` | `INSERT` and the write access types below |
| `opensearch-native` | `tools/opensearch-preview` `OBSERVED_COMPAT` | `indices:data/read/search`, `indices:data/read/mget`, each with an exact index and `observed=true` | wildcard actions, `indices:admin/`, `indices:data/write/`, `cluster:admin/`, and the action groups below |

`starrocks-native` and `doris` use the SQL permission table because `docs/database-support/04-database-profiles.md` §9.6 says StarRocks and Doris speak the MySQL protocol. The tokens are not rewritten to `mysql-native` or `mysql-mode`.

`mongodb-native` admits a permission list only when one item is `name=read`, `status=known`, `granted=true`, and no other granted name is classified below. The profile §10 names the built-in `read` role. A list with no granted `read` is `PERMISSION_UNKNOWN` even when every other role is ungranted. Names match exactly. `readWrite` is not accepted as a prefix of `read`.

MongoDB built-in roles classified as not read-only when granted: `readWrite`, `dbAdmin`, `dbOwner`, `userAdmin`, `readAnyDatabase`, `readWriteAnyDatabase`, `userAdminAnyDatabase`, `dbAdminAnyDatabase`, `clusterAdmin`, `clusterManager`, `clusterMonitor`, `hostManager`, `backup`, `restore`, `root`. Any other MongoDB name, including `SELECT`, `reader`, and `Read`, is `PERMISSION_UNKNOWN`.

`read` on a SQL compatibility mode is `PERMISSION_UNKNOWN`. `SELECT` on `mongodb-native` is `PERMISSION_UNKNOWN`. `read` on `doris` or `starrocks-native` is `PERMISSION_UNKNOWN`.

ClickHouse §9.6 names the account as a `readonly` profile, a quota, and `max_execution_time`. The vendor setting `readonly=1` allows read queries and refuses `SET`. `readonly=2` also allows `SET` and `CREATE TEMPORARY TABLE`, and it does not by itself stop `RESTORE` from writing, so `2` is not the enforced profile. Primary reference: <https://clickhouse.com/docs/operations/settings/permissions-for-queries>. The string `readonly` is a setting, not a `GRANT`, and it stays `PERMISSION_UNKNOWN` when it is submitted as a permission name.

A `clickhouse-native` report admits only when both the report and the config carry the same `native.clickhouse`, `readonly` is the exact string `1`, `max_execution_time` is a canonical positive integer string, and every quota field (`duration_seconds`, `queries`, `errors`, `result_rows`, `read_rows`, `execution_time_seconds`) is at least 1. A missing profile is `PERMISSION_UNKNOWN` on `native.clickhouse`. `SELECT` is the read grant only when `scope.database` is an exact identifier and `scope.table` is either absent (the `system.grants` shape of `GRANT SELECT ON database.*`) or an exact identifier. `*`, an empty scope, an OpenSearch `index`, or `observed` on that scope is `PERMISSION_UNKNOWN`. Granted `INSERT`, `ALTER`, `CREATE`, `DROP`, `TRUNCATE`, `OPTIMIZE`, `DELETE`, `UPDATE`, `KILL QUERY`, `ACCESS MANAGEMENT`, `SYSTEM`, `ALL`, `CREATE TEMPORARY TABLE`, `BACKUP`, `RESTORE`, and `SOURCES` are `PERMISSION_NOT_READONLY`. `SHOW DATABASES` is a companion privilege: it does not admit by itself and it does not deny when it has no index scope. The mode alone does not admit. A preview that reports `SELECT` without a scope and without `native.clickhouse` stays `PERMISSION_UNKNOWN`.

OpenSearch §10 names index read-only. The vendor action ids for that outcome are `indices:data/read/search` and `indices:data/read/mget`. Primary reference: <https://docs.opensearch.org/latest/security/access-control/permissions/>. Each admits only when `scope.index` is one exact index name, `scope.database` and `scope.table` are absent, `scope.observed` is `true`, `status` is `known`, and `granted` is `true`. `observed` means a call on that index succeeded. A role file that lists `indices:data/read/mget` while `POST /{index}/_mget` returns 403 is not that observation. Granted `false` does not block. A wildcard in the action name, an action under `indices:admin/` or `indices:data/write/` or `cluster:admin/`, and the action groups `write`, `crud`, `create_index`, `delete`, `indices_all`, `data_access`, `all_access`, and `cluster_composite_ops` are `PERMISSION_NOT_READONLY` when granted. Any other action, including `indices:data/read/get`, `indices:data/read/search*`, and the string `read`, stays `PERMISSION_UNKNOWN` or `PERMISSION_NOT_READONLY` as that rule says. The mode alone does not admit. A preview that omits `scope` stays `PERMISSION_UNKNOWN`.

`native.clickhouse` on any compat mode other than `clickhouse-native` is `PERMISSION_UNKNOWN`. Report and config `native` values that differ are `IDENTITY_MISMATCH`.

`tidb` and `elasticsearch` stay `COMPAT_MODE_UNKNOWN`. An unrecognized mode still classifies SQL permission names with the SQL table, so a `tidb` report whose grants are `SELECT`, `USAGE`, and `CONNECT` is denied only for the compat token.

`DorisScan` stays `BUDGET_UNIT_UNKNOWN`. `Rows`, `Bytes`, `Credits`, `Dbu`, `ReadUnits`, and `WallClock` stay the budget set. R1w without the existing approval stays `R1W_UNAPPROVED`.

Checklist text follows the report compat token. SQL compatibility modes keep the PostgreSQL role and `GRANT SELECT` text, including the placeholder `PASSWORD '<由 DBA 设置>'`. `mongodb-native` names `db.grantRolesToUser` for the built-in role `read` on `<database>` and verifies with `connectionStatus`. The login is the report `source_id`. The database name is the placeholder `<database>`. `starrocks-native` and `doris` use a MySQL-protocol `CREATE USER`, `GRANT SELECT`, and `SHOW GRANTS`. `clickhouse-native` states the `readonly` profile, the quota interval, and `GRANT SELECT` on the reported database or table. `opensearch-native` states `index_patterns` and the two action ids. This process still does not send the statements.

`BatchIncremental` requires CursorKind. `ChangeFeed` requires FeedKind. `Cdc` requires CdcKind. `SnapshotToken` requires TokenOrder. `Budget` requires BudgetUnit.

## Write classes

| Class | Admission |
|---|---|
| R0 | Admitted. Read-only queries. |
| R1 | Admitted. DBA-created objects or parameters. The platform only reads them. The checklist carries risk, rollback, and a verification SELECT. |
| R1w | Blocked unless the report and the config carry the same approval: `approved=true`, an ADR id starting with `DEC-` or `ADR-`, and a non-empty `audit_ref` that does not contain `PASSWORD`. Approval without an R1w request is also `R1W_UNAPPROVED`. Approval does not prove zero-write. |
| R2 | `WRITE_CLASS_R2`. The platform must not create source objects. |
| R3 | `WRITE_CLASS_R3`. No business-table writes. |

Write options, each a block:

| Option | Code |
|---|---|
| Signal table, `signal.data.collection`, or Debezium `snapshot.mode` other than `never` | `SIGNAL_WRITE` |
| Heartbeat write or `heartbeat.action.query` | `HEARTBEAT_WRITE` |
| `CREATE STREAM` | `STREAM_WRITE` |
| Trigger creation | `TRIGGER_WRITE` |

`templates/debezium-no-write.json` is a disabled template. It is not a config document and it is not permission to copy upstream example GRANTs.

## Decision

`oo.source-admission.decision/v1` fields: `protocol`, `admitted`, `matrix_mutated`, `blocks`, `model`, `identity`, `dba_checklist`, `zero_write`.

`matrix_mutated` is always false. `zero_write.proven` is always false and `zero_write.basis` is `not_audited` on this decision. Policy admission is not a source zero-write proof.

Checklist items are text for a person. This process does not send them. Each item has `risk`, `rollback`, and `verification`. Password literals are replaced with `PASSWORD '<redacted>'`. The placeholder `PASSWORD '<由 DBA 设置>'` is kept. The canary `CANARY-SECRET-DL01W102` is replaced. Native checklist wording is in Native report interpretation.

## PostgreSQL adapter

`adapt-pg` reads one existing `oo.source.pgsql/v1` document produced by `oo-connectors` `inspect_access`. Identity copies `protocol`, `core_contract`, `source_id`, and `secret_ref` and records SHA-256 of the original bytes. `FactStatus` values are the snake_case names `known`, `absent`, `unknown`, and `not_collected`.

`scan` and `bounded_read` supported become `Catalog` and `BatchFull` at level `designed`, write class `R0`, consistency `S0`. `cdc` supported or `cdc.implemented` is `CDC_NOT_IN_SLICE`. `source_write` or `source_ddl` supported is `SOURCE_WRITE_FORBIDDEN`. The frozen report has no budget unit, so the decision includes `BUDGET_UNIT_UNKNOWN` and is not admitted.

## PostgreSQL budget evidence

`oo.source.pgsql/v1` does not grow budget fields. `adapt-pg` stays on that document and still denies `BUDGET_UNIT_UNKNOWN`.

`oo.source.pgsql.budget/v1` is an additive document from `PgSession::inspect_budget` on the same session that produced the companion `oo.source.pgsql/v1` bytes. The producer returns both documents only after the access report, `pg_settings`, and selected-table privilege reads succeed. A source error returns no document.

Companion `sha256` is the lowercase hex SHA-256 of the exact access bytes, including the trailing newline. Admission hashes those bytes again. A different serialization is a different document.

The evidence records three configured limits from the session `PgBudget`: `Rows`, `Bytes`, and `WallClock` (milliseconds). `Rows` and `Bytes` are client caps: `server_enforced` is false and `server_status` is `not_applicable`. `WallClock` is `statement_timeout`. `server_enforced` is true only when `pg_settings.setting` for that name is the same millisecond value and `unit` is `ms`. A different number is `SERVER_SETTING_MISMATCH`. A missing row, a non-`ms` unit, or an unparsed value is `SERVER_SETTING_UNKNOWN`. The producer does not convert `s` to milliseconds. `lock_timeout`, `idle_in_transaction_session_timeout`, and `default_transaction_read_only=on` are recorded the same way. The client connect timeout and the client row-rate window of 50000 are not server caps. Claiming server enforcement for rows, bytes, or that window is `SERVER_CAP_MISSTATED`.

`cost_model.family` is `oltp_accounting`. `units` are exactly `Rows`, `Bytes`, `WallClock`. `estimate_confidence` is `unknown`, `estimate_basis` is `not_collected`, and `billing` is `none`. Credits, Dbu, ReadUnits, and a known estimate are `BUDGET_UNIT_UNKNOWN`. An administrator hard cap does not turn an unknown estimate into a known price.

`shadow` is `none`. `local-pg` or `local_pg` is `NATIVE_SOURCE_RETAG`. `compat_mode=postgres-native`, `compat_status=known`, and `target_id=postgres-17-native` require admission `tested`, major 17, minor 10, and a `server_version` that starts with `17.10` and does not contain `kingbase`, `opengauss`, `polardb`, `mysql`, `mariadb`, `oracle`, `vastbase`, or `highgo`. Otherwise compatibility is `unknown` and the block is `COMPAT_MODE_UNKNOWN`.

TLS admission requires `verify-full`, `caller-ca-only`, hostname verification, and `server_observation=observed`. Otherwise `TLS_UNOBSERVED`.

Permission `known` means the privilege answers were collected. Read-only admission also requires `CONNECT`, `USAGE`, and `SELECT` on every selected object, no `TEMP`, `CREATE`, or table write privilege, an empty selection is not read-only, and the access report's role, principal, membership, definer, and write facts agree. `PERMISSION_UNKNOWN` and `PERMISSION_NOT_READONLY` both block. A SQL NULL privilege is unknown.

Identity `known` requires an aligned principal copied from the access report. Empty or unknown identity is `IDENTITY_MISSING`. A present identity that disagrees with the access report or the config is `IDENTITY_MISMATCH`. `core_contract` must be `oo.config/v1` on both documents.

The driver digest is SHA-256 of the UTF-8 strings `sqlx-postgres`, `0.9.0`, and `_tls-rustls-ring-no-roots`, joined by NUL bytes. Any other kind, name, version, feature, or digest is `DRIVER_MISSING`.

`oo.source-admission.pg-budget/v1` config carries `source_id`, `secret_ref`, `target_id`, `compat_mode`, `current_user`, `database`, three `hard_caps` (`unit`, `hard_limit`, `client_cap`, `confidence`), `write_options`, `debezium`, `requested_write_class`, and `r1w`. Each hard cap uses confidence `unknown`, a non-zero `hard_limit`, and `client_cap` equal to the evidence configured limit for that unit. A missing unit is `BUDGET_UNIT_UNKNOWN`. A different number is `BUDGET_UNIT_MISMATCH`. R2, R3, R1w, signal, heartbeat, stream, and trigger denials are the same codes as `validate`.

`admit-pg-budget` writes `oo.source-admission.decision/v1`. `matrix_mutated` stays false. `zero_write.proven` stays false and `basis` stays `not_audited`. Identity `source_report_protocol` is `oo.source.pgsql.budget/v1` and `source_report_sha256` is the evidence bytes. Catalog and BatchFull follow the access report. Three Budget capabilities use level `designed`, write class `R0`, consistency `S0`, and units `Rows`, `Bytes`, and `WallClock`. This command does not emit `Cdc`.

## Audit

`oo.source-admission.audit/v1` classifies a PostgreSQL csvlog. The statement text is field index 13 and the user is field index 1. An empty reader log is `audit_empty` and is not proof. A reader verb outside the read-only set is `audit_write`. Proof requires at least one reader `SELECT` and no reader forbidden verb, with `basis=pg_csvlog`. Statements by other users are counted and are not attributed to the reader. The audit emits verb counts, not statement text.

## CLI

Binary: `oo-source-admission`.

| Command | Arguments | Exit 0 | Exit 2 | Exit 1 |
|---|---|---|---|---|
| `validate` | `--report --config` | admitted | blocked | invalid input |
| `seal-native` | `--report --config --out` | admitted and files written | blocked or seal refused | invalid input, or the output path already exists |
| `adapt-pg` | `--report` | admitted | blocked | invalid input |
| `admit-pg-budget` | `--evidence --access --config` | admitted | blocked | invalid input |
| `audit-log` | `--reader --log` | proven | not proven | invalid input |
| `observe` | `--config --password-file --ca-file --log` | proven | not proven | invalid input or connect/query failure |
| `registration-recipe` | `--validator --driver` | recipe written | — | unreadable path |

Passwords are read from files. They are not accepted on argv. `observe` opens one verify-full session, runs fixed SELECTs, then reads the csvlog file. It does not issue DML or DDL.

`oo-source-admission-pg-report` calls `PgConnector::connect` and `inspect_access`. `oo-source-admission-pg-budget` calls `inspect_budget` on that same session shape, then `admit-pg-budget` on the exact bytes, and only then a bounded read. A blocked admission does not read. Exhaustion, statement timeout, and `SnapshotOwner::cancel` before `next_batch` are separate phases. The harness is built only against the archived `oo-connectors` tree.

## Capture witness

`validate` without `capture_witness` is unchanged. An empty `capture_methods` list is still not, by itself, a block, and `identity.secret_ref` is still copied onto that decision.

`capture_witness`, when present, is an absolute path of an `oo.source-admission.capture-witness/v1` object. The object names the capturer, the Arrow stream, the driver file, and the schema and type preimage files. It carries no hash, no `admitted` bit, and no `trusted` bit. Unknown fields and duplicate keys are invalid input.

The capturer protocol is `oo.{engine}.capture-tick/v1`, or one of the producer documents below. `{engine}` has to equal the witness `source_engine` and `engine_for_compat` of the report. The witness engine names are `doris`, `starrocks`, `clickhouse`, `opensearch`, `mongodb`, `oracle`, `postgresql`, `mariadb`, and `mysql`. A compat token outside that map is `ENGINE_UNSUPPORTED`. A capturer `engine` field, when present, has to equal the witness engine. `oo.oracle.c5-producer-profile/v1` is not a capturer. `sealed_import_grade_requested` is not a grade.

Producer protocols are `oo.oracle.c5-producer-catalog/v1` (`oracle`), `oo.mariadb.c5-capture-catalog/v1` (`mariadb`), `oo.postgres-preview.capture-check/v1` (`postgresql`), and `oo.mysql-customer.audit-receipt/v1` (`mysql`). A capture tick still requires `label`. A producer binds the snapshot when `snapshot.value`, `position`, or `snapshot_sha256` equals the witness value. `snapshot.kind`, when present, has to match. `snapshot.kind` `token` is not the boolean `snapshot_token`.

Grade, method, and delete detection are read from the selected table, then from the capturer once when the table omits the field. The keys are `consistency` or `consistency_grade`, `route` or `capture_method` or `method`, and `delete_mode` or `delete_detection`. The method token is `C1`, `C2`, `C3`, `C4`, or `C5`. The only alias is `c1-logical`, which is `C1`. `c4_partition`, `c5_diff`, and `c1_cdc` are not methods. The delete token is only `reconcile`, `partition_replace`, `unknown`, `none`, or `key_absent`. The derived method is kept when a write, cursor, or arrow check fails. Quality is a string or a one-element array. `DELETES_LAGGED` does not change the grade. That quality together with grade `T` is `GRADE_CONTRADICTION` and the grade is left as written. `partition_metadata_recorded` is not a grade.

A table is `tables[].name` or `tables[].table`. Both keys present and unequal is not a match. When `tables` is absent and `logical_table` equals the witness table, the document is the table fact. Schema is `database`, `logical_schema`, or `owner` on the capturer or that table. An index-only grant names the source object, not the logical table. It matches when `index`, `source_index`, `source_object`, or `approved_index` on that table or the capturer equals `scope.index`. Those fields have to agree with each other. When none of them is present, one exact index in `reader_audit` paths is that source object. A second index, or a grant with no recorded source object, is `OBJECT_UNAPPROVED`. A grant that also sets `database` or `table` still requires `scope.index` to equal the logical table.

On a capture tick, `product_reader_write_attempts` and boolean `snapshot_token` have to be present. Absent is `WRITE_UNOBSERVED`. A non-zero write attempt is `WRITE_OBSERVED`. `snapshot_token` true is `SNAPSHOT_TOKEN`. On a producer, absent `product_reader_write_attempts` is still `WRITE_UNOBSERVED`, except `product_read` `zero`, which is an observed zero. `unreadable` is not zero. `logminer_used`, `native_loader`, and `sealed_import_grade_requested` are not write audits. Absent boolean `snapshot_token` on a producer is not `WRITE_UNOBSERVED` and is not recorded as false. A seal requires that boolean to have been observed. The genuine PostgreSQL native-page exported snapshot exception below records true; other sources still require false. `cursor_safety`, when present, has to be `no_cursor`. A capture tick and producer methods `C1`, `C2`, and `C3` require `no_cursor`. Producer `C4` and `C5` do not invent a cursor.

Row count on a capture tick is the capturer `rows` array length and the sum of Arrow record-batch lengths. A `row_count` number is not used by itself. On a producer with no `rows` array, `row_count` is accepted only when it equals the Arrow record-batch length. If it is present and differs, the block is `ROW_COUNT`. A capture tick binds Arrow by the `arrow` file name. A producer with an `arrow` name uses that same rule. `arrow_sha256` null is `ARROW_UNBOUND`. A hex `arrow_sha256` has to equal the Arrow file. When that field is absent, the witness path is the binding and the row count still has to match.

`schema_digest` is SHA-256 of the schema preimage file. `arrow_schema_sha256` is SHA-256 of the first Arrow IPC schema message metadata, without the 8-byte continuation prefix. The stream is not an Arrow file (`ARROW1` is refused). Those two digests are different fields. Equal digests are `SCHEMA_DIGEST_COLLISION`. `type_digest` is SHA-256 of the type preimage file. `config_digest` is SHA-256 of the config file. `input_digest` is SHA-256 of the Arrow file. `driver_digest` is SHA-256 of the driver file and has to equal `report.driver.digest`. A caller-supplied hash is not read.

Nullable follows the Arrow slot. An absent nullable slot is the format default false, and it has to equal the preimage bool. An absent Date unit is the Arrow format default DAY. Decimal `bitWidth`, when the slot is absent, is the format default 128. Precision and scale are copied onto the binding only when the preimage states them and they equal the Arrow values. `key` and `document` are copied only when the preimage states them.

FloatingPoint precision DOUBLE is `float64`. HALF and SINGLE stay unsupported. Timestamp uses the Arrow default SECOND when the unit slot is absent. The accepted names are `timestamp_second_naive`, `timestamp_second_utc`, `timestamp_millis_naive`, `timestamp(ms,utc)`, `timestamp_micros`, `timestamp_micros_utc`, and `timestamp_nanos_utc`. A zone other than UTC is accepted only for microsecond timestamps, and the timezone string is copied. Dictionary encoding accepts signed Int32 indices of Binary values as `dictionary_int32_binary`; declared dictionary IDs must have an IPC DictionaryBatch before record batches, and undeclared dictionary IDs are refused. List is one child: signed non-null int64 is `list_int64`, binary is `list_binary`, and utf8 is `list(utf8)`. A field named by capturer `unclassified` is `SCHEMA_UNSUPPORTED`. `not_approved` stays a type-preimage token. It is not rewritten, and it does not admit `where_point` or any other absent field.

The public decision is still `oo.source-admission.decision/v1`. `identity.secret_ref` is omitted. `secret://` and a `secret_ref` key are blocks when they occur in the capturer or a preimage, and they are not copied into the decision. The report and the config may still carry the approved reference.

A non-empty `capture_methods` list that disagrees with the derived method is `CAPTURE_UNTRUSTED`. The model then carries the derived method. An empty list is filled from the capturer.

## Native seal

`seal-native --report --config --out DIR` writes `decision.json`, `admission-receipt.json`, `registry.json`, and `catalog.json` only after the public decision is admitted and the engine is a native seal engine. The directory is not created on a block, a refusal, or invalid input. An existing output path is left untouched.

Seal engines are `oracle`, `clickhouse`, `opensearch`, `doris`, `mongodb`, `starrocks`, `mariadb`, `postgresql`, `mysql`, and `sqlserver`. A budget whose unit is not `Rows`, or whose hard limit is 0, prints `budget unit` and exits 2. A decision that is not write class R0 prints `not r0` and exits 2. `validate` does not apply those three seal refusals. A MariaDB decision is sealed only when that source's own capturer admits. A Doris capturer on a MariaDB witness is `PROTOCOL_UNSUPPORTED` and does not seal.

The registry is `oo.source-admission.binding-registry/v1` with one `oo.source-admission.capture-binding/v1` object. The binding names `driver_path` and `driver_sha256`. `driver_sha256` equals `driver_digest`, which is SHA-256 of that file. `driver_kind`, `driver_name`, and `driver_version` repeat the report driver. Committed `open_server_registration` opens `driver_path` and refuses a file larger than 32 MiB. `catalog-import-native` checks the binding digests and does not open the driver file.

`registration-recipe --validator PATH --driver PATH` writes `oo.source-admission.registration-recipe/v1`. `trusted` is false. `allowlist_updated` is false. `approved_by_consumer` is true only when the validator SHA-256 is one of `a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2`, `ae7dc727181ae101bab84e328d39c38c3fdd6613255f79062404cbc9c8069577`, or `e465241fc1292b6eb5a13cf12efbffe360a9fbf1cbb03c02affb0241a4e58e1f`. An unlisted driver longer than 32 MiB is `exceeds_open_server_registration`. The published exact pairs are Doris `d5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c` at 115155008 bytes and OpenSearch `874d2588595d63b597139358b47e3097fcc5f45f772ffb5000ecd371e8e88b2e` at 116863584 bytes. Those pairs are `approved_driver_resource`. A different length for the same digest is not. The quota owner and the lease owner are DL01-W1-06. `lease_issued` is false. `grade_source` is `capturer`. `self_written_grade_accepted` is false. `receipt_registry_sha256` is the preimage. `oo_native_binding_registry_sha256` is the full registry file hash. This command does not edit the consumer allowlist, does not issue a lease, and does not say that a consumer process has already imported a package.

The admission receipt is `oo.source-admission.capture-receipt/v1`. It records the derived `capture_method`, `consistency`, `delete_detection`, `write_attempts`, and `snapshot_token`. It does not record `admitted` or `trusted`. `admission_receipt_sha256` on the binding occurs once and is SHA-256 of the receipt file. `registry_sha256` on the receipt is SHA-256 of the registry bytes after that stamp is replaced by 64 zero hex digits. The full registry file hash is a different value and is what `OO_NATIVE_BINDING_REGISTRY_SHA256` names. `registry_preimage` compares the receipt field to the preimage. The historical Doris consumer ELF `4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe` compared the receipt field to the full file hash. Committed `registry_preimage` compares the receipt field to the preimage. `approved_admission_bin` allows `a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2`, `ae7dc727181ae101bab84e328d39c38c3fdd6613255f79062404cbc9c8069577`, and `e465241fc1292b6eb5a13cf12efbffe360a9fbf1cbb03c02affb0241a4e58e1f`. The catalog is `oo.pipeline.sealed-import-catalog/v1` and carries the same four digests. It is an input catalog, not the PostgreSQL OID preimage `bind_native` recomputes later.

Exit codes match `validate`: 0 admitted and, for `seal-native`, files written; 2 blocked or seal refused; 1 invalid input. A seal refusal can exit 2 while the decision says `admitted` true. No source password is read.

## Out of scope

Matrix promotion, `verified` and `certified`, a 14-day nightly, families other than this PostgreSQL 17.10 fixture, ledger updates, and coordinator acceptance. Author completion is provisional until a separate review.

## Native 2026-10-12 assurance boundary

Frozen Linux ARM64 Rust 1.94 ELF SHA-256 `0e18dec38d41d04b00650c613902e4ff21ffab700795208916679699718ca90f` is retained at `/tmp/oo-dl01w1-02-native-admission-0e18dec`. The consumer owner must register that exact digest; registration-recipe reports the older published allowlist and does not authorize a new ELF. `capture_receipt_sha256` hashes the actual capturer JSON, and is the native update request `audit_digest`. It does not mean the raw platform audit file hash. A capturer can retain descriptive source audit path/hash/statistics. Admission validates the capturer write observation and hashes that document; it does not independently parse heterogeneous source SQL audit files. Source owners must retain and validate their actual raw platform audit. Public decision `zero_write.proven=false` and `basis=not_audited` remain honest. The generic C1 `no_cursor` witness contract is not sufficient for an actual WAL-position producer; preserve its original producer route and do not invent cursor absence.

## Original source type and position extension

Linux ARM64 Rust1.94 successor ELF `/tmp/oo-dl01w1-02-native-admission-final2` SHA256 `f3353f1d13e68990693818660cb9eec180961dde66dd39f84d94ef7d80366081` supports ten closed engine mappings including SQL Server `sqlserver`/`sqlserver-mode`. Real SQL Server source remains untested without its actual authorized instance. Time64 MICROSECOND/NANOSECOND tokens are `time64_micros`/`time64_nanos`, preserving actual Arrow unit and bitWidth64. FixedSizeBinary12/16 tokens are `fixedsizebinary(12)`/`fixedsizebinary(16)`. Mongo original `union(dense,0:utf8,1:int64)` validates dense mode, exact typeIDs0/1 and two original typed children; `struct(kind:utf8,n:int32)` validates exact child names/types. Children nullability remains captured in the original schema metadata hash. Decimal tokens `decimal128(p,s)`/`decimal256(p,s)` are accepted only when p/s equal actual Arrow precision/scale and the preimage precision/scale agree.

`oo.postgres-preview.native-page-capture/v1` is a PostgreSQL-only actual producer protocol. Root logical_schema/logical_table, row_count, arrow+arrow_sha256, source identity and all own observed grade/write/deletion facts are required by the same binding. C1 `cursor` is `{codec,position}` and position must equal actual snapshot.value. `pg-commit-lsn/v1` with pg_commit_lsn accepts a positive decimal position; `pg-export-snapshot/v1` with pg_snapshot accepts the original three-part hex exported snapshot. The real exported snapshot accepts and preserves observed snapshot_token=true; arbitrary true flags still fail SNAPSHOT_TOKEN. It does not imply certified transactional capture, applied barrier verification, or independent source audit. Missing or unequal cursor is refused; no_cursor is not invented. `native` delete detection is retained as an observed token. Actual snapshot.value takes precedence over descriptive phase label; labels remain unchanged.

## Original R1 / empty stream / row budget contract

Frozen Linux1.94 ARM64 ELF `/tmp/oo-dl01w1-02-native-admission-43bde81c`, SHA256 `43bde81c78726c9c3aed6bbb9e2d5eaf0a34f3b45d00fb861886f7841d161523`, adds narrow original R1 for actual `oo.postgres-preview.native-page-capture/v1` C1 and `oo.mssql-preview.capture/v1` C2. The config requests R1, capabilities are R0/R1, capture is derived R1, r1w is absent and all platform creation/write options remain false. Arbitrary R1 routes remain refused. Truthful PostgreSQL known granted REPLICATION is accepted only when the same report/config permissions and fully source-bound native C1 DBA context agree; table-write privileges remain blocked.

Capturer root `dba_objects` has exactly two objects: PG `publication`/`replication_slot`; SQL Server `ct_database`/`ct_table`. Each records exact name, `created_by=dba`, `observed=true`, `platform_created=false`, `platform_write_attempts=0`, nonempty risk/rollback, and absolute `observation_path` plus `observation_sha256`. Admission opens nonempty raw observation and recomputes its hash. This hashes real source-owner observation bytes; admission does not independently parse arbitrary platform SQL audit or certify DBA provenance. Receipt preserves `write_class`, all descriptors, and actual `source_cursor`; model uses C1 LogicalDecoding or C2 ChangeTracking/Sequence with actual opaque snapshot or monotone position ordering.

SQL C2 per-table capturer binds original `candidate_position` CT protocol, source identity/database GUID/recovery fork/database, exact selected schema/name/object_id/schema_sha256/unique keys/version. Snapshot kind is mssql_ct_version and value is actual decimal selected version. Prior position, when present, must have identical identity/object/schema/keys and not exceed current version. Actual TDS query observation is required. No source instance or CT provenance is invented.

Schema-bearing zero-row record batches are valid, including no-change polls/delete-only batches and a full reconciliation after last-row deletion. A batch must still be present and exact observed rowcount must match; an absent Arrow RecordBatch.length is format default0. Arrow Decimal.scale absent slot is format default0 (actual MySQL unsigned DECIMAL(20,0)). Actual stream rows exceeding configured Rows hard_limit block BUDGET_EXCEEDED. This checks returned stream rows, not server quota enforcement or aggregate multi-page window; source owner retains actual enforced budget tests.

Final43 actual PG snapshot and delta authoritative seals exit0; source-owned all21 pages were also sealed with earlier1628 ELF and retained raw source evidence. Negative bad DBA observation hash, source INSERT grant, and unapproved scope are rejected2 with no output. MySQL actual8 stream no longer has schema/type unsupported after scale0 correction, but its old rowcount/Arrow-name metadata still refuse; it is not declared admitted. SQL Server live C2 source remains pending actual authorized instance.

## Final original source metadata binding

Final ELF `/tmp/oo-dl01w1-02-native-admission-minimal`, SHA256 `0b584453877a4c5ec3e7c311c199ade8af723229ea2ab56745858e37f7a1e274`, preserves PG native source OID semantics. Native page preimage columns require source_oid(u32), source_typmod(i32), source_format(string), exactly matching observed root source_columns entries name/three metadata fields/arrow_type/nullable and the type preimage. These fields are PG-only and propagate into registry fields; UUID2950 is never inferred from arbitrary binary16 or column name.

MySQL oo.mysql.capture-tick/v1 may bind actual row_count without duplicating all rows in JSON, still equal to actual Arrow row sum. Valid schema-only Arrow streams with zero batches are zero rows (actual absent ClickHouse C4 partition); all source witness/scope/budget/audit checks remain. SQL captured C2/R1 accepts its original absolute/file:// private reference only when both policy references agree, full actual R1 objects validate, and referenced path is a regular0600 file (no credential reads). Generic validate remains managed secret:// only.

Ponytail author verification: final Linux1.94 suite/Clippy/release0; actual PG typed snapshot/delta seals0, OID tamper refuses2/noout; actual MySQL capture9 all5 tables seal0 with actual runtime ELF. SQL and ClickHouse actual serializer empty streams pass targeted schema tests; SQL live source/CT authority still absent. No additional matrix, wrapper, dependency or review was added. Token ratio is unknown because per-agent usage is unavailable.

## Source-bound C4 and BSON fields

For ClickHouse C4, `capturer.partition_scope` is required and copied into `registry.bindings[0].partition_scope` only after checking the exact source id, generation, schema/table, sorted complete observed partition ids, UUID table epoch, nonempty source schema fingerprint, mutation-free state, exact schema/type/Arrow SHA-256 digests and Arrow row count. `current_signature` is opaque and must begin with `partition_id|`; null requires zero rows. The source cannot assert `previous_signature`. The consumer must read the authoritative target ACK and compare epoch, prior signature and the complete partition set before replacement and durable ACK. A source-only observation file is not an ACK.
The ClickHouse capture tick may state a numeric `row_count` when the actual Arrow stream has the same number of rows. The ordered bootstrap reference uses capture/import control generations 1, 2 and 3 for three disjoint partitions of one unchanged source observation; these numbers are not a source CDC position.

For MongoDB, source replay can provide every column's sorted `bson_types` variants and `bson_present_rows`; schema and type preimages must agree, counts cannot exceed Arrow rows, and a singular `bson_type` must equal the sole observed variant and match Arrow storage. Mixed string/integer union and object/null struct combinations are checked explicitly. The singular tag is preserved at `registry.bindings[0].fields[*].bson_type`. Semantic conversions are restricted to `{ "variant": "07" }` on fixed binary 12 (ObjectId), `{ "variant": "05", "subtype": "04" }` on fixed binary 16 (UUID), and `{ "variant": "05", "subtype": "00" }` on binary (ordinary bytes). Other tags retain their existing Arrow type. Untagged historical fields remain untagged; consumers must not infer semantic conversion from names or widths. Admission checks the fixed producer's hashed preimages and capturer, but does not reopen the original raw BSON; that per-row proof belongs to the approved source producer and is recorded separately.
