# PostgreSQL bounded full-snapshot stream `oo.source.pgsql.stream/v1`

This is the pull on one already-open snapshot owner. Acceptance is qualified to the PostgreSQL full-mode snapshot, the bounded pull, and `RunComplete` admission. Typed key chunks are `reader-keys-v1`. Active-pull cancellation is `reader-lifecycle-v1`. Resume cursors, CDC, and the registry API stay unsigned. Catalog `type_support.converted` stays false. `connector_capabilities` `scan` and `bounded_read` are supported for that path. `cdc`, `type_conversion`, `registry_api`, `source_ddl`, and `source_write` stay unsupported.

The stream reuses admitted columns, `decode_payload`, and `assemble_batch`. It does not change type mapping or canonical cell semantics.

## Caller

`SnapshotOwner::open_stream` takes a `StreamBudget` and moves the owner's two sessions into one pull. `next_batch` is the only driver. It returns `StreamItem::Batch`, `TableEof`, or `Exhausted`. `SnapshotOwner::finish` returns `SnapshotFinished` as lifecycle close only. `SnapshotOwner::finish_complete` returns `RunComplete` as full-selection admission. `TableEof` is the end of one table. `Exhausted` means every selected table has reached `TableEof` and the owner is still open.

`StreamBudget::try_new` requires `batch_rows` in `1..=65536`, `batch_bytes > 0`, `max_value_bytes > 0`, and `max_row_bytes >= max(2, max_value_bytes)`. Invalid combinations are `config` facts `batch_rows`, `batch_bytes`, `max_value`, or `max_row`. `queue_batches` is 1 and is not a parameter. These fields are not part of `config_identity`.

`config.max_rows` still caps catalog statements. It does not stop the scan. `config.max_bytes` is the cumulative decoded field-payload cap across accepted rows.

## One cut

One owner, one `REPEATABLE READ READ ONLY` transaction, one current-catalog guard. Every selected table uses that same run identity. Before each table the pull re-checks the current catalog and the guard. A mismatch is the existing typed drift or privilege failure. There is no `fetch_all`, no whole-table collect, no `OFFSET`, and no new snapshot per table or per batch.

`has_column_privilege` in the repeatable-read recheck, and the same function prepared on the guard session, can still answer with the grant from open after a committed revoke. The current ACL is that same `READ COMMITTED` `CURRENT_CATALOG` statement. The statement already returns `SESSION_USER` and `CURRENT_USER`. On each selected column of the same row it also returns whether `SELECT` is present in that row's table ACL (`relacl`, or the table default when the ACL is null) or column ACL (`attacl`). The check counts `PUBLIC`, roles `CURRENT_USER` can use, a superuser, and `pg_read_all_data`. The guard compares the role facts with the required principal. A selected column whose privilege is not true is `Privilege` / `select_not_granted`, before the named portal opens and before the scan. The open transaction, generation, and cut stay in place. The pull does not reconnect or open another session.

A shape or primary-key difference remains `schema_drift`. A committed change that keeps the shape and leaves selected-column `SELECT` true does not fail the guard. A table with no selected column has no privilege cell in that observation. `SQLSTATE 42501` from a scan that starts after the guard is still a failure, including that case. It is the race after that observation, and it is not turned into success. A row already decoded for the current table remains that table's one pending batch. The next table's guard runs on the demand that would open it. After that guard returns `select_not_granted`, the pull does not scan the revoked table and does not emit its batch. The observation's row reservation and `LIMIT` are unchanged. The privilege expression adds no row.

`next_batch` polls the only drive. That drive writes `Execute` with a positive allowance of one row while the caller is waiting. There is no background reader and no unbounded channel. A batch of `batch_rows` may contain that many Executes inside one demand. The next demand is what starts the next Execute. The portal stays open between data batches of the same table.

## Keys

A table with no primary key, a primary key the probe cannot use, or a primary key of 9 to 32 columns still opens. `read_probe` is unchanged: those keys still return `probe_key` and poison the owner. A primary key wider than PostgreSQL `INDEX_MAX_KEYS` (32) is `config` fact `selection` and does not open. An empty column list with a non-empty primary key is `probe_key`. `plan.key` is set only for a probe-supported key. The stream does not invent a resume key. `StreamBatch.resumable` and `TableEof.resumable_key` are false. A key digest on a supported key is the existing canonical digest, not a cursor.

`open_stream` still scans every selected table with the full-snapshot statement: no `WHERE`, no `ORDER BY`, and no `OFFSET`. A keyed chunk and `read_keys` use a different statement on that same owner and the same repeatable-read transaction. The statement orders by the declared key and binds half-open bounds. It is not a resume cursor. See `reader-keys-v1`. The portal Parse/Bind and an Execute already sent are cancelled on the guard session with `pg_cancel_backend`, scoped by the data backend's pid and `backend_start`. The pull keeps that future pinned until it returns or `CLEANUP_OBSERVATION_BOUND_MS` passes. See `reader-lifecycle-v1`.

## Empty and zero-column tables

A table with columns and no rows produces no `Batch`. The first `Execute` completes with no row. That demand returns `TableEof` with `rows_emitted = 0`.

A table with no columns, and a table whose every column is omitted by the preserve-row policy, uses `SELECT 1 FROM schema.table`. The constant is not an Arrow column. `TableEof.zero_column` is true only when the source column list is empty. `TableEof.columns` is the lossless Arrow column count. The presence value is int4: field payload 4, DataRow payload `2 + 4 + 4 = 10`. Those 4 bytes count toward `batch_bytes`, `max_value_bytes`, and `max_bytes`. They are not an Arrow cell. `RecordBatch` for that table has zero columns and a row count equal to the presence rows.

## Receive bounds

After the table re-check, and before the named portal is opened, the pull installs `PgIncomingBounds`. `max_message_len` is `max(max_row_bytes + 4, 8192)` so a small row cap still accepts `RowDescription`. `max_value_len` is `max_value_bytes`. `max_row_payload` is `max_row_bytes`. `max_columns` is the result width (1 for a presence scan).

The driver refuses an oversize frame before the body is reserved. `IncomingWire` limit kinds map to `budget` facts `max_message`, `max_value`, `max_row`, and `max_columns`. Any other incoming-wire fault is `protocol` fact `incoming_wire`. `Display` is the static fact. It does not include server bytes or the value. A row that cannot start an empty batch under `batch_bytes` or `max_bytes` is `budget` fact `max_bytes` and is not emitted. A further row that does not fit a non-empty batch is held, one row only, and the current batch is emitted with no further `Execute`.

A wire fault or a statement-timeout drop leaves the connection unsynchronized. The pull does not `ROLLBACK` that connection. Shutdown closes the socket without `ROLLBACK`. A synchronized portal is closed and receive bounds are cleared. Successful table completion keeps the owner transaction and snapshot open. A statement failure aborts that transaction; owner cleanup releases it with the existing full `ROLLBACK` path. The stream does not emit `SAVEPOINT`, `ROLLBACK TO`, or `RELEASE`, and does not roll back an active portal.

The client deadline is checked before a table opens, between Executes, and while a portal open or Execute is in flight. A deadline during that in-flight work cancels that backend and returns `budget` fact `deadline`. A deadline with rows already buffered still emits that batch; the next demand returns `deadline` when no row is waiting. `statement_timeout` around one portal open or Execute cancels that backend and returns `budget` fact `statement_timeout`. Caller cancel returns `protocol` fact `cancelled`. The `PgBudget` statement and lock caps stay at most 120 seconds. The run-deadline default constant is 300 seconds and does not raise those caps.

## Accounting

Counts on each item are cumulative for the pull:

- `executes`: `Execute` invocations, not catalog or transaction-control statements
- `reserved_rows` and `consumed_rows`: the per-Execute row reservation and the driver's consumed count
- `decoded_payload_bytes`: accepted field payloads, nulls as 0, not prefixes and not Arrow
- `arrow_preimage_bytes`: the existing Arrow preimage length
- `arrow_assembly_bytes`: sum of `RecordBatch::get_array_memory_size`
- `retained_batches`: 0 once the item is returned
- `retained_rows`: 1 while one decoded row is held for the next batch
- `wire_view_capacity_peak`, `wire_accepted_logical_payload`, `wire_assembly_bytes`: scalar peaks from `PgIncomingAllocation`

Those wire fields are sums of live `BytesMut` view capacities and copy counts. They are not RSS, not `malloc_usable_size`, and not backend heap. The portal holds the connection, so the pull samples them only when the portal is not borrowed: at pull start and after the portal is closed. Mid-portal peaks are not available. The event log is not copied.

## Release

`finish`, `abort`, `cancel`, and `Drop` stop the pull, close a still-open portal, and release both backends through the existing cleanup path. `Drop` sets the cancel flag and schedules that work on the current Tokio runtime. Dropping `next_batch` puts the drive back and does not by itself end the backend. The known driver panic on a shrinking receive view is an a129 dependency. This stream does not modify the driver and does not claim that gate.

## Frozen no-savepoint implementation update

The stream cleanup above follows implementation `4055237dfeb6df672388419dd3889a6fcaa4d356` and resource receipt `a3cd94c2c4337528342c8ece8d3ec142e2d8b2fc`. The actual PostgreSQL 90,000-row proof and 193,937 reader statements contain zero forbidden statements, zero `SAVEPOINT`, and zero `RELEASE`. Cancellation and deadlines return SQLSTATE `57014`. This is author-complete evidence for the stream and bounded-window paths; it does not sign off every connector path, the source product, or the parent. `read_probe` still emits `SAVEPOINT oo_probe_lock` and was not called by this proof; `pg_stream_acl_live` was compiled and checked but not executed against the database in this packet. Earlier savepoint behavior and its failed source audit remain recorded in the original fixed evidence.
