# Findings

## Measured pull

The green `pg_stream_live` run on PostgreSQL 17.10 printed:

`executes=13 reserved_rows=13 consumed_rows=9 decoded_payload_bytes=59 oracle_payload=59`

The independent oracle was read before the fixture writer committed. Parent multiset was the four seeded rows. Child multiset was the three seeded rows, including NULL. The empty table contributed a `TableEof` with `rows_emitted=0` and no data batch. The zero-column table contributed two presence rows and `zero_column=true`. Decoded Arrow payload bytes equalled the oracle sum of 59. Consumed rows equalled the nine oracle rows. Reserved rows equalled executes. The catalog open uses `max_rows=128` because the reserved-keyword statement returns about 78 rows, so an open budget of 8 cannot admit the owner. That budget does not prove a naive scan would have stopped at 8. The scan proof is the full oracle and one execute advance per demanded batch.

Arrow assembly bytes were 5528 and the Arrow preimage was 132. Those are decoder and `RecordBatch` sizes. They are not process RSS and not backend heap.

Wire samples on that run: view capacity peak 8192, accepted logical payload 12561, assembly bytes 951021. `PgNamedPortal` holds the connection, so the meter is read only before the first execute of a table and after the portal closes. Mid-portal samples are unavailable. The figures are sums of `BytesMut` views over the connection lifetime, including catalog traffic. They are not RSS, `malloc_usable_size`, or backend heap.

## Suspended portal looks active

A page is `Execute` plus `Flush`. There is no `Sync` and no `ReadyForQuery` while the portal is suspended. On this server the backend stayed `state=active` with `wait_event=ClientRead` for the whole time the test held a batch. The first live run required `state=idle in transaction` and panicked with `backend 274 stayed active` after the oracle line had already been recorded. The retained check is `ClientRead` or `idle in transaction`, plus an unchanged `oo-pg-source` log across the idle wait. The green bounds run kept the log line count at 1051 during 400 ms and recorded 1052 after the next demand. Executes moved from 1 to 2 only on that demand.

## Bounds

`max_value_bytes=4` on a text value `abcdef` returned budget fact `max_value`. The display was `source error: budget fact=max_value` and did not contain the value. `max_row_bytes=9` on the int4 row whose payload is 10 bytes returned fact `max_row`. The display did not contain the row text. Both refusals released the owned backends. Incoming-wire limit facts are `max_message`, `max_value`, `max_row`, and `max_columns`. Other incoming-wire faults map to protocol `incoming_wire`.

## Fixes that the green run already includes

- Portal open stores the error or the held portal and only then touches the connection. Matching on the portal `Result` while using the connection in the error arm did not compile (`compile-fail-1.exit` 101).
- `Placed` is compared with `matches!`. `close` returns `PgPortalClose`, and the owner maps that to `()`.
- Test credentials pass the password string, and Arrow arrays import `Array`.
- `StreamItem::Batch` and the drive-end value are boxed so Clippy `large_enum_variant` is satisfied. The demand wait uses one `if let` chain.
- The bounds activity row does not keep an unread user column.

## Left as they were

The a129 receive-view shrink panic is unchanged. Vendor `buffered.rs` on the archive still hashes to `f41e0e4dd6baca062fb332ab66da3f5e9c0600fd`. This run used finite fixture tables. It does not pass a large-stream or corrected-driver gate.

`type_support.converted` stays false. Connector capability flags were not edited. R3 key and cancel coverage, R4, and whole-reader acceptance are not signed by this run.

The two `sqlx-postgres` unfulfilled `dead_code` expectations are the frozen vendor sources. They do not fail `-D warnings` for this package, and they were not patched.
