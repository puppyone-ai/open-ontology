# cdc-transaction v1

Bounded assembler for one explicit normalized PostgreSQL CDC feed.
Contract name: `cdc-transaction-v1`.

This crate is the first CDC product unit. It assembles caller-supplied
`bind`, `barrier`, `begin`, `data`, `end`, and `rollback` records.
It does not read Debezium, PostgreSQL, a socket, a replication slot, or
Valkey. It does not write a source, a target, an offset, or a heartbeat.
It does not ACK. `applied` is always JSON `null`.

`upstream_commit_lsn_verified`, `cross_table_consistent`, `target_applied`,
`ack_allowed`, `debezium_normalizer`, `p2_verified`, and `p11_verified` stay
false. A later unit must bring a real normalizer, P2, P11, a fenced target
merge, and durable offsets before any of those can change.
A transaction larger than the bounds below fails `needs_spool` and is not
truncated. Durable spool of a transaction over 200000 events is a later unit.

Pinned dependencies, copied from the repository root lock through the
already resolved `tools/session-transition` graph. The package name in this
lock is the only edit. No crate was added. `cargo update` was not run.

| crate | version | checksum |
|---|---|---|
| serde | 1.0.229 | `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` |
| serde_core | 1.0.229 | `67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48` |
| serde_derive | 1.0.229 | `e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348` |
| serde_json | 1.0.151 | `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14` |

No path dependency. `oo_core::Position` is not linked here because `oo-core`
uses workspace inheritance and this crate is a separate workspace.
The Linux proof parses every frozen position string and the sample timestamps
with the real `oo_core::Position` and `oo_core::rfc3339::Rfc3339Timestamp`.
`rust-version` is 1.94.0. Edition 2024. Safe Rust only.

## Feed

UTF-8 JSONL. One JSON object per line. LF separates lines. A CR is
malformed. The line body, excluding the LF, is at most 1 MiB
(1048576 bytes). The byte that would make it longer is not parsed and is
not stored. Compact or spaced JSON is accepted. Duplicate object keys,
including decoded `\u` escapes, are malformed. Unknown fields are malformed.
There is no `row_lsn`, `lsn`, or other per-row position field. A row LSN is
never a commit position.

The first record must be `bind`.

## Bind

```json
{"kind":"bind","source":"billing","lineage":"<64 lowercase hex>","epoch":1,"selected_tables":["public.lines","public.orders"],"schema_hash":"<64 lowercase hex>","config_hash":"<64 lowercase hex>","type_hash":"<64 lowercase hex>","hook_hashes":[],"commit_position_provenance":"caller_declared"}
```

`source` matches `^[a-z][a-z0-9_]{0,62}$`.
`selected_tables` has 0 to 256 qualified names
`^[a-z][a-z0-9_]{0,62}\.[A-Za-z_][A-Za-z0-9_]{0,127}$`, sorted, unique.
Hashes are 64 lowercase hex digits. `hook_hashes` has 0 to 32 entries,
sorted and unique. Empty means the caller hashed an empty hook list.
`commit_position_provenance` is only `caller_declared`.
`epoch` is an integer `0..=4294967295`.

## Barrier

An epoch change or a lineage change is a barrier. Positions are not compared
across the barrier. The encoded position still has to be unique across
distinct transactions in the whole feed, because the decimal does not
contain the lineage.

`{"kind":"barrier","reason":"epoch","epoch":2}`

The epoch must differ. `lineage` must be absent. Open transactions must be
absent.

`{"kind":"barrier","reason":"reset","lineage":"<64 lowercase hex>","epoch":1}`

The lineage must differ. The epoch is the new epoch and may repeat a number.
Open transactions must be absent.

## Begin, data, end, rollback

`{"kind":"begin","txn_id":"inv17"}`

`txn_id` matches `^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,127}$`.
A second begin for an open id is idempotent. An id that already completed
or rolled back is `txn_id_reuse`.

```json
{"kind":"data","txn_id":"inv17","global_txn_sequence":1,"data_collection_sequence":1,"table":"public.orders","key":[{"type":"int","ordinal":0,"value":"17"}],"op":"upsert","payload":[{"type":"text","ordinal":0,"value":"open"},{"type":"null","ordinal":1},{"type":"unavailable","ordinal":2}],"unavailable_ordinals":[2]}
```

`op` is `upsert` or `delete`. Both sequences are integers `>= 1`.
`table` is a qualified name. It does not have to be selected.
Every source event is required, including events for tables that are not
selected, so the end count can be proved. Only selected tables are emitted.

Cells are opaque typed values. Ordinals are `0..=32767`, strictly increasing.
At most 4096 cells. `null` is a known SQL null. `unavailable` is a TOAST
placeholder and carries no value. `unavailable_ordinals` is exactly the
unavailable payload ordinals, strictly increasing. A null ordinal is not
unavailable. Key cells are required, and a key cannot be null or unavailable.
`int` is an optional minus and digits, no sign on zero, no leading zero, at
most 76 digits. `decimal` is that integer form plus an optional fraction of
at least one digit; trailing fractional zeros are significant; a negative
zero is rejected. `bytes` is even-length lowercase hex. `text` is a JSON
string. `bool` is a JSON boolean.

Arrival order inside a transaction may differ from sequence order.
The set of `global_txn_sequence` must be exactly `1..=count` before the
transaction can complete. Each collection's `data_collection_sequence`
must be exactly `1..=that collection's count`. A hole, a zero, or a value
above the declared count refuses completion. The same global sequence, or
the same collection sequence on one table, with the same canonical body is
idempotent and does not change the count. The same ordinal with a different
body is `duplicate_conflict` and does not replace the stored event.

The event hash is lowercase hex SHA-256 of the compact preimage, fields in
this order: `data_collection_sequence`, `global_txn_sequence`, `key`, `op`,
`payload`, `table`, `txn_id`, `unavailable_ordinals`. Cell objects are
`ordinal`, `type`, then `value` when a value exists. SHA-256 is the FIPS
180-4 function in this crate. Empty input is
`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
`abc` is `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

```json
{"kind":"end","txn_id":"inv17","count":3,"per_collection_counts":{"public.lines":2,"public.orders":1},"commit_lsn":"16/B6E40","source_commit_timestamp":"2026-04-15T08:30:00.123456Z"}
```

`count` is the number of distinct data events, selected and unselected.
`per_collection_counts` names exactly the tables that appeared. A missing
or extra name is `collection_unknown`. A declared sum that is not `count`,
or a collection whose stored events do not match its declared count while
the transaction is otherwise over-full, is `collection_count_gap`.
The sum uses checked `u64` addition. Overflow is `count_overflow` and does
not wrap. An end whose stored events are still short of `count` is
`end_too_early`. The stored events stay pending. The transaction is not
complete.

`commit_lsn` is the caller-declared transaction commit LSN. It is not read
from a data record. Canonical form is `HIGH/LOW`, each side `0` or hex
without a leading zero, uppercase, 1 to 8 digits. Nine digits, lowercase,
a prefix, a mask, and a second slash are `commit_lsn_invalid`. The value is
`(high << 32) + low` with checked shifts. No 32-bit mask is applied.

The source commit timestamp is the canonical spelling produced by
`oo_core::rfc3339::Rfc3339Timestamp`: `YYYY-MM-DDTHH:MM:SSZ` or the same
with a fraction of 1 to 6 digits and no trailing zero, offset only `Z`,
Gregorian year `0001..=9999`, no leap second.

`{"kind":"rollback","txn_id":"inv17"}` drops that open transaction and its
journal charge. It does not emit a complete transaction and does not move
`complete_decoded`. A second rollback of that id is idempotent. Rollback of
a completed id is `rollback_after_complete`.

## Position

PostgreSQL position is `epoch << 64 | commit_lsn` as a non-negative scale-0
decimal of at most 38 digits, the `oo_core::Position` rule. The shift and
the add are checked. `u32` epoch and `u64` LSN fit in 29 digits. A result
that did not fit would be `commit_position_overflow` and would not wrap.

Hand value used by the smoke probe and the tests: epoch `1`, LSN `16/B6E40`.
`0xB6E40` is `749120`. `0x16 << 32` is `94489280512`. The LSN is
`94490029632`. The position is `18446744168199581248`.

Inside one barrier segment the next position must be strictly greater than
the previous completed position. A smaller value is
`commit_position_regression`. An equal encoded position on a different
transaction, including after a barrier, is `commit_position_conflict`.
Complete transactions are emitted in end order, not begin order and not
row-LSN order. Events that share one commit LSN keep their own
`global_txn_sequence`. The assembler does not collapse an earlier upsert
because a later event has the same key.

## Progress

`read` counts lines and bytes presented to the assembler.
`seen` counts accepted begin, data, end, rollback, barrier, and bind
records, plus identical duplicates. Seen is not applied.
`complete_decoded` counts transactions whose end matched. Its last position
is the encoded decimal of the last completed transaction in this feed.
`applied` is null. `ack_allowed` is false.

EOF with an open or refused transaction sets `resume_required` and does not
drop the pending events. Completed transactions already emitted stay emitted.
A clean feed, including a rollback of every open transaction and a
transaction whose selected row count is 0, has `resume_required` false.
Exit 0 is that clean feed. Exit 3 is EOF with `resume_required` and no
refusal. Exit 2 is a refusal, including when `resume_required` is also true.
Exit 1 is usage or I/O. Nothing in this crate ACKs.

```json
{"contract":"cdc-transaction-v1","kind":"refused","fault":"end_too_early","txn_id":"inv17"}
```

`txn_id` is JSON null when the refusal is not tied to one transaction.

## Budgets

| bound | value | refusal before the excess is stored |
|---|---|---|
| open transactions | 16 | `open_set_exceeded` |
| data events in one transaction | 200000 | `needs_spool` |
| data events in the open journal | 200000 | `needs_spool` |
| opaque key and payload value bytes in one transaction | 33554432 | `needs_spool` |
| those bytes in the open journal | 33554432 | `needs_spool` |
| JSONL line body | 1048576 | `line_too_long` |
| remembered rolled-back ids | 200000 | `needs_spool` |

The byte budget is the stored length of key and payload values. `null`,
`unavailable`, and `bool` contribute nothing. The canonical preimage is only
the event-hash input. It is not this budget: a minimal preimage is already
larger than `33554432 / 200000`, so charging it would make the event cap
unreachable. The two caps are independent.

A transaction of exactly 200000 events can complete when its value bytes
also stay within 33554432. The next data event returns `needs_spool` and is
not stored. No event is dropped to make room. No partial payload is kept.
No ACK is set.

## Sample

Selected tables `public.lines` and `public.orders`. Three data events:
order `17` upsert, line `1` delete, line `1` upsert with a null cell and an
unavailable cell. End count 3, `public.orders` 1, `public.lines` 2, commit
LSN `16/B6E40`, timestamp `2026-04-15T08:30:00.123456Z`. The decoded
transaction has three rows, sequences 1 then 2 then 3, position
`18446744168199581248`, and every attestation false.

## CLI

`cdc-transaction` reads JSONL on stdin and writes JSONL on stdout.
No arguments. Each newly completed transaction is one `complete_transaction`
object. A refusal writes one `refused` object. The last line is `progress`.

## Out of scope

Debezium images, topic routing, SMT, offset history, heartbeats, Valkey
ACK and trim, target COPY, fencing, group commit, snapshot fill, and
durable large-transaction spool. P2 and P11 are not claimed.
