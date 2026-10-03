# PostgreSQL typed key plan `oo.source.pgsql.keys/v1`

This plans and reads typed half-open chunks on one already-open snapshot owner. It is not whole-reader acceptance. Cancellation of an in-flight Execute is `reader-lifecycle-v1`. Resume cursors, CDC, and the registry API stay unsigned. Catalog `type_support.converted` stays false. `connector_capabilities` and `bounded_reader` stay unchanged.

## Caller

`SnapshotOwner::plan_keys(target_rows)` runs in the open repeatable-read transaction. `target_rows` must be `1..=50000`. Any other value is `config` fact `target_rows` and does not poison the owner. `DEFAULT_TARGET_ROWS` is 50000. `MAX_PLAN_CHUNKS` is 64.

`SnapshotOwner::open_chunk` pulls one `PlannedChunk` through the existing `next_batch` drive. `SnapshotOwner::read_keys` pulls one `KeyRange` the same way. Both refuse a second stream with `stream_active`. `close_stream` restores the two sessions without ending the transaction, so the next chunk can open on the same cut.

`SNAPSHOT_DEADLINE_DEFAULT` is 300 seconds. The caller passes the `Instant`. This owner does not start that clock and does not raise the `PgBudget` statement or lock cap. Those stay at most 120 seconds.

## Declared key

The declared key is the primary key when it has 1 to 8 columns, every column is `NOT NULL`, every column is bool, int2, int4, int8, text, varchar, or bpchar, every column has a lossless plan, and the primary index is neither partial nor an expression (`indpred` and `indexprs` are null).

Otherwise the plan uses the first non-primary unique key, in index-name order, that meets those same column rules and whose index is neither partial nor an expression. A primary key of any other type, a nullable unique column, a partial index, or an expression index is not a declared key. The table is then one unkeyed chunk.

`read_probe` is unchanged. An unsupported primary key still returns `probe_key` from that API.

## Planning

Keyed planning reads `count(*)` as one metadata row, then the first key with `ORDER BY` the key and `LIMIT 1`, then each next boundary. The next boundary is the last row of `WHERE key > previous ORDER BY key LIMIT stride`, taken with an outer descending limit of 1, plus the page count. There is no `OFFSET` and no arithmetic split of a text or composite key. PostgreSQL compares the row. The statement has no `COLLATE` clause, so the column collation applies.

`stride` starts at `target_rows`. When `ceil(count / target_rows)` is greater than 64, `stride` becomes `ceil(count / 64)`. Each planning fetch is armed for one row. The walk stops when the page is empty or shorter than `stride`. That short page stays in the current chunk. A boundary equal to the previous boundary is `protocol` fact `key_plan`. The walk also returns `key_plan` if it would emit more than 64 chunks.

Chunks are half-open. The first lower bound is the first key. Each later lower bound is the previous chunk's upper bound. The last upper bound is unbounded. An empty keyed table is one chunk with both bounds empty. An unkeyed table is one chunk, `keyed` false, both bounds empty, `row_count` and `stride` empty. `PlannedChunk.resumable` is false. The chunk carries generation and input digest. It does not carry SQL.

## Reads

`open_chunk` requires this owner's generation and input digest. It resolves the declared key again in the same transaction. A width, oid, or keyed-flag mismatch is `protocol` fact `key_plan` and poisons the owner. `open_chunk` on an unkeyed chunk uses the full-snapshot statement, including a presence scan when every output column is omitted.

`read_keys` with both bounds empty is a full ordered scan of the declared key. A table with no declared key returns `protocol` fact `key_required` and does not poison the owner. A bound must be empty or the declared key width. Values are bound parameters. The scan statement has no `LIMIT`, no `OFFSET`, and no `COLLATE`.

`open_stream` is not this plan. It still reads every selected table with no key predicate and no `ORDER BY`. `StreamBatch.resumable` and `TableEof.resumable_key` stay false. A key digest on a declared key is the existing canonical digest, not a cursor.

The same repeatable-read transaction covers planning and every chunk. A commit from another session after the owner opened is absent from later chunks. `SnapshotFinished` is still the only successful close. Output from a cut that did not finish is incomplete. A later generation is a new cut and reads the committed rows again. Cancelling an Execute that has already been sent is `reader-lifecycle-v1`, on this same owner.
