# REPORT — full-mode production stream design

Status: design only. R1 review is still pending. Nothing here approves that review, authorizes a vendor edit, or implements `read_table`. `read_probe` stays a point probe. R3 still owns typed key ranges, backend cancellation proof, and whole-round recovery. This design leaves those hooks in place. CDC, incremental cursors, and multi-node snapshot stay out of this mode.

Input tree: `2b6dd57a0dc3d36e098268638c0364af247d07fb` / `9c75b25cf7e539cc043c7c90ee750aad9a2c3ccc` / parent `e5a066622e0409762fe44f11c4d303d2468a3290`. Bindings are in `FINDINGS.md`.

## 1. Where the bytes have to be stopped

The allocation that matters is `ReadBuffer::reserve` of an inbound message, which happens inside `PgStream::recv_unchecked` before `DataRow::decode_body` and before `decode_row`'s `charge`. The production stream has to stop there.

Three sizes are different objects:

| Object | What grows | Bound that actually limits the heap |
|---|---|---|
| One field, including a detoasted text/bytea/jsonb/array | The Int32 length in the current `DataRow`, read before that field's bytes | `max_value_bytes`, checked before `read_buffered(length)` |
| One row of many small fields | Sum of those lengths plus 2 + 4×columns, one message | `max_row_wire_bytes`, checked before the field that would cross it |
| A batch and the consumer queue | Owned cells, `assemble_batch`'s clone, Arrow 60 arrays, and any queued batch | Page size and a pull stream with no prefetch task |

A SQL predicate, `LIMIT`, `max_rows`, or `charge` after `as_bytes` does not limit the reserve. `Execute.limit = 0` in `PgConnection::run` asks the server for every row and then closes the unnamed portal in the same pipeline. The stream must not call `run`, `fetch`, or `fetch_all`.

The server still detoasts before it writes the field (`printtup` → `slot_getallattrs` → `SendFunctionCall`). The client bound does not cap backend `palloc`. Acceptance measures the client path.

## 2. Vendor API that can enforce the reserve

No connector-side check can see the length first. `PgConnection`'s socket is private. The patch stays inside the existing Rust driver. It does not add `unsafe`, a C parser, or a second protocol stack. `PgConnection::run` keeps `limit: 0` and the immediate `Close` so catalog, guards, and `read_probe` keep their current behavior.

Add these items under `crates/oo-connectors/vendor/sqlx-postgres` and a counter hook in `sqlx-core`'s `ReadBuffer`. Default bounds stay unset, so unbounded callers keep today's reserve.

```text
pub struct PgIncomingBounds {
    pub max_message_len: u32,   // Int32 length field, includes itself
    pub max_value_len: u32,     // one DataRow field payload, not the 4-byte prefix
    pub max_row_payload: u32,   // sum of field payloads + 2 + 4*natts
    pub max_columns: u16,
}

pub enum PgIncomingLimit { Message, Value, Row, Columns }

pub struct IncomingHighWater {
    pub reserved_additional: u64,
    pub capacity_after: u64,
    pub messages: u64,
}
```

`PgIncomingLimit`'s `Display` is a static label. It carries no bytes and no server text. `err_protocol!` is the wrong carrier: it appends file and line, and `map_sqlx` collapses every `Error::Protocol` to fact `protocol`.

`ReadBuffer::reserve` records `additional` and `available.capacity()` after `BytesMut::reserve` into `IncomingHighWater`. `PgConnection::incoming_high_water()` returns a copy. That counter is the allocation measurement. A test that asserts `if length > max` without reading this counter only mirrors the branch.

New methods, all `pub` on `PgConnection`, none of them used by `run`:

1. `set_incoming_bounds(&mut self, bounds: Option<PgIncomingBounds>)`.
2. `prepare_extended(&mut self, sql: &str) -> Result<PgStatement, Error>` using the existing prepare path.
3. `bind_named_portal(&mut self, statement, arguments, formats: binary) -> Result<PortalId, Error>`. The portal is named (`sqlx_p_…`), not `PortalId::UNNAMED`. Bind, Flush, wait for `BindComplete`, and stop. Do not send `Close` or `Execute`.
4. `execute_portal_row(&mut self, portal, limit: NonZeroU32) -> Result<PortalStep, Error>` where `PortalStep` is `Row(PgRow)`, `Suspended`, or `Complete`. The first call sends `Execute { limit }` and Flush. Later calls read one already-requested message. `DataRow` uses the bounded reader below. `PortalSuspended` and `CommandComplete` leave the connection able to accept `Sync`.
5. `sync_ready(&mut self) -> Result<TransactionStatus, Error>` sends `Sync` and reads `ReadyForQuery` only when the previous step was `Suspended` or `Complete`. It must refuse when a `DataRow` body is still unread.
6. `close_named_portal(&mut self, portal)` after `sync_ready`.
7. Existing `close_hard` remains the unsynchronized shutdown.

Bounded `DataRow` read, used only when bounds are set and the type byte is `D`:

1. Header already in hand (5 bytes). If `message_len < 4` or `message_len > max_message_len`, return `PgIncomingLimit::Message` and do not `Continue(expected_len)`.
2. `read_buffered(2)` for `natts`. `natts > max_columns` → `Columns`. Do not read field bodies.
3. For each column, `read_buffered(4)`. Length `-1` is NULL. Length `< -1` is a protocol error. Length `> max_value_len` → `Value` before `read_buffered(length)`. Running payload sum `> max_row_payload` → `Row` before that `read_buffered`.
4. Accepted field bytes append into one `BytesMut` that is never asked to grow past `max_row_payload`. Then build the existing `DataRow` so `PgRow::try_get_raw` still borrows it.

Non-`DataRow` messages used by this pager (`BindComplete`, `RowDescription`, `CommandComplete`, `PortalSuspended`, `ReadyForQuery`, `ErrorResponse`, `NoticeResponse`, `ParameterStatus`) take the same `max_message_len` check before reserve. A too-long notice or error is `Message`. The body is not decoded and is not logged. Today's `recv` notice logger stays on the old path; the pager must not call it for a rejected frame.

On any `PgIncomingLimit`, the socket is desynchronized. The method sets an internal `protocol_sync = false` and returns. It does not call `wait_until_ready`. The connector then `close_hard`.

`PATCH.md` gains Hunk 3 describing this API, the default-off behavior, and the statement that `run` is unchanged. No version number is presented as crates.io identity. No `Cargo.toml` or `Cargo.lock` change is part of the hunk.

This patch is a coordinator handoff. It is not authorized by this analysis or by the prepared connector-only file list.

## 3. Stream on the R1 owner

`SnapshotOwner` already holds the one connection and the open `REPEATABLE READ READ ONLY` transaction. The stream borrows that owner. It does not connect, `BEGIN`, or `COMMIT`. Drop of the stream does not `finish` the owner.

`read_probe`, `PgSession::catalog`, and `PgSession::read_typed` stay as they are. The probe can still allocate one unbounded `DataRow` before `charge`. That residual is R1's probe, and this design does not rename it into a batch stream.

### 3.1 Plans and SQL

Use the `BoundTable` captured in `open`: `plans` from `plan_columns` (`map_column`, `guard_plans`, `load_type_graph` on this transaction), `select_sql` / `schema_sql` / `table_sql` from `quote_ident` that already passed `valid_quoted_ident`. Re-check `valid_quoted_ident` and call `guard_plans` again before prepare. Build one statement:

```text
SELECT <quoted lossless columns> FROM <quoted schema>.<quoted table>
```

No `LIMIT`, no `ORDER BY`, no bound values, no `OFFSET`. `compose_select` in `types/read.rs` appends `LIMIT $1` and is the wrong template. A table with only unsupported non-key columns uses `SELECT 1 FROM …` the way `compose_presence` does, still without a limit, and every row decodes as an empty lossless cell list. `FailRead` still fails in `guard_plans` before prepare.

`decode_payload` and `assemble_batch` are called unchanged, per cell and per full page. `RunBinding` uses this owner's `generation` and `input_digest`. Result oids use the same `accepts_result_oid` check as `decode_row`. Text format is `text_format_refused`. The batch protocol string stays `oo.source.pgsql.types/v1`.

Before the first `Execute` of a table, rerun the owner's node, relation, attribute, and privilege recheck. A mismatch poisons the owner with the existing fact (`schema_drift`, `select_not_granted`, `identity_drift`, `read_node`, `write_capable`, `privilege_unknown`). No batch is yielded.

### 3.2 Pull pages

No spawned reader and no channel. `oo-connectors` does not depend on `tokio::sync` or `futures-core`. The producer runs inside `poll` / `next_batch` on the caller's task, on the owner's connection.

State machine per table:

1. `Ready`. On poll, debit the rate bucket (section 5), check the deadline, `Execute` with `limit = batch_rows` (`NonZeroU32`, never zero).
2. `InPage`. Read one bounded row at a time into a page builder. Drop the `PgRow` after `decode_payload` returns. A field or row limit returns `Err`, drops the builder, sets `protocol_sync = false`, poisons, and does not yield the partial page.
3. Page closes when `batch_rows` or `batch_bytes` or `max_decoded_batch_bytes` is reached, or the portal suspends / completes. A full page under the caps is yielded, then `sync_ready` so the backend is idle in the same transaction with the named portal still open. Phase returns to `Ready`.
4. `CommandComplete` yields the last under-cap page if it has rows, then a `TableEof` event, closes the portal, and `sync_ready`. The next poll on a finished table returns `None` only after `TableEof` was yielded.
5. Empty table: one `TypedBatch` with zero rows and the real plans, then `TableEof { rows: 0 }`.

`batch_rows` and `batch_bytes` end a successful page. They are not errors. Crossing `max_value_bytes` or `max_row_wire_bytes` is an error and discards the open page.

Events:

```text
enum RunEvent {
    Batch(TypedBatch),
    TableEof { schema, table, batches, rows },
}
```

`Ok(None)` is the run EOF and is produced only after every selected table has yielded `TableEof` with no error. `Err` is terminal. Batches already polled are not a successful cut: the owner is poisoned, `finish` returns that poison rather than `SnapshotFinished`, and the generation cannot be reopened or appended. There is no resume cursor in the batch. A missing key does not synthesize one. A present key is not used to page in this mode; R3 adds the range predicate on the same pager.

### 3.3 Several tables, one cut

`read_run(limits)` walks `binding` table order, which is the admitted selection order. One portal at a time. The next table's prepare starts only after the previous portal is closed and `sync_ready` returned in the same transaction. A writer that commits between table A and table B is invisible to this transaction. The stream does not reconnect and does not open a second snapshot to "finish" the run.

`read_table` is the same pager for one bound table and is what a test calls. A run that stops after one table of a multi-table owner has not reached run EOF.

### 3.4 Deadline, drop, and the R3 hook

The deadline is the owner's existing `Instant`. Expiry before an `Execute` or between pages is fact `deadline`, poison, no `TableEof`, no run EOF. The implementation does not start a new per-table transaction and does not keep a consistent flag.

Phase is visible to later R3 work as `Ready` or `InPage`, plus `protocol_sync`, plus the existing `BackendKey` (`pid`, `backend_start`). R2 does not open a second connection and does not send `CancelRequest`.

- Drop or error in `Ready` with `protocol_sync`: leave the owner open. `abort` can `ROLLBACK` on a synchronized connection. Drop does not return `SnapshotFinished` or `SnapshotAborted`.
- Drop or limit error in `InPage`, or any `PgIncomingLimit`: `protocol_sync = false`. `finish` and `abort` call `close_hard` and return fact `cleanup_failed`. They do not return `SnapshotFinished` or `SnapshotAborted`. They do not call `wait_until_ready`, because that would reserve every remaining frame. R3 replaces this branch with cancel-by-`BackendKey` and the `pg_stat_activity` proof. The 5000 ms observation constant stays R1's cleanup bound, not an R2 claim that the backend has exited.

`idle_in_transaction_session_timeout` is the statement timeout (≤ 120 s) while the pager sits in `Ready`. A consumer pause longer than that is a server abort, mapped by the existing SQLSTATE `57014` path to fact `statement_timeout`, then poison. R2 does not raise that GUC. A page itself is one `Execute` of at most `batch_rows` rows whose wire sum is at most one frame cap per row, so the statement timer resets per page.

## 4. Defaults

`StreamLimits` is a new type, checked in `try_new`. It is an argument to `read_run` / `read_table`. It is not a field of `PgBudget` and it is not hashed into `oo-pg-config-identity/v1`.

| Knob | Default | Hard maximum | Why |
|---|---:|---:|---|
| `max_connections` | 4 | 4 | Docs 03 example `limits.max_connections`. This owner already uses one connection. |
| `max_rows_per_sec` | 50000 | 50000 | Docs 03 example. Burst equals `batch_rows` and is labeled. |
| Snapshot deadline | the owner's `Instant` | caller-supplied; prepared initial default 300 s | Docs 04 `max_snapshot_txn_seconds`. Expiry does not split the cut. |
| `statement_timeout` / idle-in-transaction | existing `PgBudget`, ≤ 120 s | unchanged | `SESSION_GUARDS`. |
| `batch_rows` | 1024 | 8192 | Docs 03's 50000 is the later chunk target, not an Arrow batch. Docs 17 caps each `RecordBatch` separately. `Execute.limit` is this number. |
| `batch_bytes` | 4 MiB wire field bytes | 8 MiB | Under the 32 MiB inspection ceiling. Ends a page. |
| `max_decoded_batch_bytes` | 8 MiB | 16 MiB | Counts owned scalar bytes as cells are pushed, before `assemble_batch`'s clone. Covers text×2, jsonb tree, and the array cap. |
| `max_value_bytes` | 1 MiB | 1 MiB | One detoasted field, before its body is read. |
| `max_row_wire_bytes` | 2 MiB | 2 MiB | The row sum. Also `max_message_len`. |
| `max_columns` | 1600 | 1600 | Matches the server's heap attribute ceiling. The protocol field is still a `u16`. |
| `queue_batches` | 1 | 1 | The yielded batch is the only retained page. The next `Execute` waits for the next poll. |

`try_new` rejects a zero Execute limit, a value cap above the row cap, a row cap above 2 MiB, and a column cap above 1600. Connector code sets `PgIncomingBounds` from these numbers for the pager only. Catalog `fetch_all` during `open` stays on `run` so a legitimate catalog text up to the 32 MiB inspection budget is not forced through the 2 MiB row cap. That catalog path remains an after-the-fact `charge`. The stream does not use it.

Resident design ceiling while a consumer holds one batch and the producer is idle in `Ready`: one decoded page (≤ 8 MiB) plus `assemble_batch`'s clone and the Arrow arrays built from it (another copy of the scalars, so plan on ≤ 16 MiB of scalar bytes plus Arrow validity/offset buffers) plus the 8192-byte read buffer. The next page does not start until the next poll. Two live batches occur only if the caller keeps the previous `TypedBatch` and polls again. Callers that need one batch of RSS drop the previous batch first. The high-water counter, not this paragraph, is the acceptance measurement.

## 5. What the rate bucket counts

Process-local token bucket keyed by source id. Rate `max_rows_per_sec`, burst `batch_rows`. Refill from elapsed time inside the poll, and sleep with the existing `tokio` time feature. A wait that would pass the deadline returns `deadline` and does not `Execute`.

Debits, all in the same bucket:

| Class | R2 amount |
|---|---|
| Data rows placed into a yielded page | one each |
| Rows of the per-table recheck and of portal prepare/describe | one each, when those statements return |
| Rows already fetched during `open` | a new `control_rows` counter on the owner, debited once at `read_run` start |
| Key planning | 0. The debit function takes a `Plan` class so R3 can charge later |
| Retries | 0. A failed stream does not reconnect or retry |

`open` increments `control_rows` in the snapshot's own fetch wrappers. It does not change admission results. The generation set is not a live-connection count. A separate `LIVE_OWNERS` count increments after a successful connect in `open` and decrements in `release_session` and `Drop`. `read_run` returns fact `connection_budget` when that count for the source id is above `max_connections`. It does not open another socket. Changing `open` itself to refuse the fifth owner is a behavior change to R1 and is listed as unresolved.

## 6. Masked facts

Existing `SourceError` display stays kind, static fact, and SQLSTATE. New facts, all static:

| Fact | Kind | When |
|---|---|---|
| `wire_frame` | Budget | `PgIncomingLimit::Message` before reserve |
| `value_limit` | Budget | `PgIncomingLimit::Value` before the field body |
| `row_wire` | Budget | `PgIncomingLimit::Row` before the field that crosses the sum |
| `column_count` | Budget | `PgIncomingLimit::Columns` |
| `connection_budget` | Budget | live owners above `max_connections` |
| `decoded_batch` | Budget | decoded scalar bytes would pass `max_decoded_batch_bytes` |
| `cleanup_failed` | Protocol | already exists; also the unsynchronized `close_hard` result |
| `deadline` | Budget | already exists |
| `statement_timeout` | Budget | already exists, including SQLSTATE `57014` |
| `snapshot_incomplete` | Protocol | already exists; stream use after release |

Driver text, notices, values, passwords, and CA PEM stay out of `Display` and out of the batch. A decode reject that the type contract already masks (`MaskedReject`, wire `value_len`, no sample) still follows `type-conversion-v1.md` for values that fit the frame and fail the decoder. A frame that does not fit is not a masked reject inside a successful batch. It is `value_limit` or `row_wire` and a poisoned owner.

## 7. Acceptance that can be preregistered

Nothing in this list was executed. One later Linux arm64 Rust 1.94 run, one builder, one private PG 17.10 fixture, side writer on a different role. OO's `application_name` stays `oo-pg-source`. The side session records zero DML/DDL from that backend on success, limit failure, slow consumer, and absent consumer.

Measurement, tied to the path in `FINDINGS.md`:

- Read `IncomingHighWater.capacity_after` and `reserved_additional` after the call returns. Pass condition for an over-limit field: `capacity_after` is below the field length, and `reserved_additional` for that read is the 4-byte length prefix (plus the earlier accepted prefix of the row, which is ≤ `max_row_wire_bytes`), not the field length.
- Sample `/proc/self/status` `VmHWM` in the test process before `Execute` and after the error. The delta has to stay under the field length when the field is larger than `max_row_wire_bytes`. This is a supplement. The counter is the allocation-path record. An assertion that repeats `max_value_bytes` is not evidence.
- Queue occupancy is the number of `TypedBatch` values the test still holds plus one if `InPage` is set. The implementation retains no extra page. Connection occupancy is `LIVE_OWNERS` for that source, expected 1.

Cases:

1. One over-limit value. Side writer inserts one toasted `text` or `bytea` of `max_value_bytes + 1` (and a second row of `max_row_wire_bytes + 1` as a single field). Stream returns `value_limit` or `row_wire` before any `RunEvent::Batch`. `finish` is not `SnapshotFinished`. High-water rule above.
2. Many columns. 1600 `int2` columns, all under the row cap, full row count matches the side oracle. A 1601-column result is `column_count` with high water at the header plus the `natts` integer. 200 `text` columns of 20000 bytes (4_000_000 bytes) returns `row_wire` at the crossing field; `capacity_after` stays ≤ bytes accepted before that field.
3. Nulls, no primary key, two tables. Requires the admission change in `HANDOFF.md`. Nullable nulls, a no-key table, and a keyed table. Independent oracle is a side `REPEATABLE READ` read taken while the side writer is blocked, then the writer commits, then OO's stream is compared as a multiset of canonical hashes per table. Order is not required on the no-key table. Table B's hashes match the blocked cut, including rows the writer changed after OO finished table A. Zero OO writes.
4. Slow consumer. Poll one batch, sleep 2 s (under the 120 s idle timeout), poll the next. `LIVE_OWNERS` stays 1. High water stays ≤ one row cap. A second case sleeps past `idle_in_transaction_session_timeout` and expects `statement_timeout`, poison, and no `TableEof`.
5. Absent consumer. Build a table larger than ten row caps, call `read_run`, do not poll, sleep, and compare `VmHWM` with a control that did not call `read_run`. Growth past one row cap fails the case. Drop without poll leaves `Ready`. `abort` may confirm rollback. A second case cancels a poll that has entered `InPage` and expects `cleanup_failed`, not `SnapshotFinished` and not `SnapshotAborted`.
6. Decoder gold. For rows that fit, canonical hashes and Arrow 60 schema from the stream match `decode_payload` / `assemble_batch` / `record_batch` on the same bytes, including the existing signed-zero, jsonb, numeric, null, and array cases in the type contract. No second decoder. `read_probe` on a small keyed row still returns the same canonical hash for that row.
7. Partial output. A failure on page 2 yields no `TableEof` and no run `None`. `finish` returns the poison.

## 8. Later Linux smoke and fixed review

Not run here. When a future dispatch freezes an implementation commit, the first smoke is the shared contract in `execution/prompts/linux-msrv-focused-checks.md`: pinned linux/arm64, `RUSTUP_TOOLCHAIN=1.94.0`, one task builder, global max two including host cargo, one task PG 17.10 fixture. `cargo fetch --manifest-path Cargo.toml --locked`, then `cargo test` and `cargo clippy --all-targets -- -D warnings` for `-p oo-connectors` with `--locked --offline`. Record image digest, `rustc -vV`, exits, and the high-water lines. No full dual-architecture matrix.

Independent fixed review reads that immutable commit, the vendor hunk if it was separately authorized, and these acceptance logs. Review of this analysis is not that review. Target apply stays blocked until R4, and only after that review. Supply and license gates stay where `a110-functional-development-gate.md` left them.
