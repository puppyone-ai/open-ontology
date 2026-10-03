# PostgreSQL bounded full-snapshot stream `oo.source.pgsql.stream/v1`

R2-S only. This is the pull on one already-open snapshot owner. It is not whole-reader acceptance. R3 key and cancel, R4, resume cursors, CDC, and the registry API stay unsigned. Catalog `type_support.converted` stays false. `connector_capabilities` is unchanged.

The stream reuses admitted columns, `decode_payload`, and `assemble_batch`. It does not change type mapping or canonical cell semantics.

## Caller

`SnapshotOwner::open_stream` takes a `StreamBudget` and moves the owner's two sessions into one pull. `next_batch` is the only driver. It returns `StreamItem::Batch`, `TableEof`, or `Exhausted`. `SnapshotOwner::finish` is still the run finish. `TableEof` is the end of one table. `Exhausted` means every selected table has reached `TableEof` and the owner is still open. `SnapshotFinished` is only `finish` after a clean close.

`StreamBudget::try_new` requires `batch_rows` in `1..=65536`, `batch_bytes > 0`, `max_value_bytes > 0`, and `max_row_bytes >= max(2, max_value_bytes)`. Invalid combinations are `config` facts `batch_rows`, `batch_bytes`, `max_value`, or `max_row`. `queue_batches` is 1 and is not a parameter. These fields are not part of `config_identity`.

`config.max_rows` still caps catalog statements. It does not stop the scan. `config.max_bytes` is the cumulative decoded field-payload cap across accepted rows.

## One cut

One owner, one `REPEATABLE READ READ ONLY` transaction, one current-catalog guard. Every selected table uses that same run identity. Before each table the pull re-checks the current catalog and the guard. A mismatch is the existing typed drift or privilege failure. There is no `fetch_all`, no whole-table collect, no `OFFSET`, and no new snapshot per table or per batch.

`next_batch` polls the only drive. That drive writes `Execute` with a positive allowance of one row while the caller is waiting. There is no background reader and no unbounded channel. A batch of `batch_rows` may contain that many Executes inside one demand. The next demand is what starts the next Execute. The portal stays open between data batches of the same table.

## Keys

A table with no primary key, a primary key the probe cannot use, or a primary key of 9 to 32 columns still opens. `read_probe` is unchanged: those keys still return `probe_key` and poison the owner. A primary key wider than PostgreSQL `INDEX_MAX_KEYS` (32) is `config` fact `selection` and does not open. An empty column list with a non-empty primary key is `probe_key`. `plan.key` is set only for a probe-supported key. The stream does not invent a resume key. `StreamBatch.resumable` and `TableEof.resumable_key` are false. A key digest on a supported key is the existing canonical digest, not a cursor.

## Empty and zero-column tables

A table with columns and no rows produces no `Batch`. The first `Execute` completes with no row. That demand returns `TableEof` with `rows_emitted = 0`.

A table with no columns, and a table whose every column is omitted by the preserve-row policy, uses `SELECT 1 FROM schema.table`. The constant is not an Arrow column. `TableEof.zero_column` is true only when the source column list is empty. `TableEof.columns` is the lossless Arrow column count. The presence value is int4: field payload 4, DataRow payload `2 + 4 + 4 = 10`. Those 4 bytes count toward `batch_bytes`, `max_value_bytes`, and `max_bytes`. They are not an Arrow cell. `RecordBatch` for that table has zero columns and a row count equal to the presence rows.

## Receive bounds

After the table re-check and `SAVEPOINT oo_stream_lock`, and before the named portal is opened, the pull installs `PgIncomingBounds`. `max_message_len` is `max(max_row_bytes + 4, 8192)` so a small row cap still accepts `RowDescription`. `max_value_len` is `max_value_bytes`. `max_row_payload` is `max_row_bytes`. `max_columns` is the result width (1 for a presence scan).

The driver refuses an oversize frame before the body is reserved. `IncomingWire` limit kinds map to `budget` facts `max_message`, `max_value`, `max_row`, and `max_columns`. Any other incoming-wire fault is `protocol` fact `incoming_wire`. `Display` is the static fact. It does not include server bytes or the value. A row that cannot start an empty batch under `batch_bytes` or `max_bytes` is `budget` fact `max_bytes` and is not emitted. A further row that does not fit a non-empty batch is held, one row only, and the current batch is emitted with no further `Execute`.

A wire fault or a statement-timeout drop leaves the connection unsynchronized. The pull does not `ROLLBACK` that connection. Shutdown closes the socket without `ROLLBACK`. A synchronized portal is closed, bounds are cleared, then `ROLLBACK TO SAVEPOINT oo_stream_lock` and `RELEASE SAVEPOINT oo_stream_lock` run. The portal is closed before that savepoint is released. An active portal is not rolled back.

The client deadline is checked before a table opens and between Executes. It does not cancel an Execute already sent. That remains R3. A deadline with rows already buffered emits that batch; the next demand returns `budget` fact `deadline`. A deadline with an empty batch returns that fact immediately. `statement_timeout` around one `Execute` is `budget` fact `statement_timeout`.

## Accounting

Counts on each item are cumulative for the pull:

- `executes`: `Execute` invocations, not catalog or savepoint statements
- `reserved_rows` and `consumed_rows`: the per-Execute row reservation and the driver's consumed count
- `decoded_payload_bytes`: accepted field payloads, nulls as 0, not prefixes and not Arrow
- `arrow_preimage_bytes`: the existing Arrow preimage length
- `arrow_assembly_bytes`: sum of `RecordBatch::get_array_memory_size`
- `retained_batches`: 0 once the item is returned
- `retained_rows`: 1 while one decoded row is held for the next batch
- `wire_view_capacity_peak`, `wire_accepted_logical_payload`, `wire_assembly_bytes`: scalar peaks from `PgIncomingAllocation`

Those wire fields are sums of live `BytesMut` view capacities and copy counts. They are not RSS, not `malloc_usable_size`, and not backend heap. The portal holds the connection, so the pull samples them only when the portal is not borrowed: at pull start and after the portal is closed. Mid-portal peaks are not available. The event log is not copied.

## Release

`finish`, `abort`, and `Drop` stop the pull, close a still-open portal, and release both backends through the existing cleanup path. `Drop` schedules that work on the current Tokio runtime. The known driver panic on a shrinking receive view is an a129 dependency. This stream does not modify the driver and does not claim that gate.
