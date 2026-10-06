# cdc-spool v1

Durable spool for one explicit normalized PostgreSQL CDC transaction.
Contract name: `cdc-spool-v1`.

This crate stores one source, one lineage, one epoch, and one transaction
from a `cdc-transaction-v1` feed. The feed grammar is the contract in
`tools/cdc-transaction/CONTRACT.md` at commit
`87c927b218cb2b6ca9b1759854a4c0fd778e8ffc`
(`e6eb4d6e88c4c26b88dd10758d144eaf30d97cd4667e465e3687bc6625108bf7`).
Cells stay opaque. This crate does not convert business values, read
Debezium, PostgreSQL, a socket, a replication slot, or Valkey, and it does
not write a source, a target, an offset, or a heartbeat. It does not ACK.

`upstream_commit_lsn_verified`, `cross_table_consistent`, `target_applied`,
`ack_allowed`, `debezium_normalizer`, `p2_verified`, and `p11_verified` stay
false. `applied` is JSON `null`. A later fenced target writer and a real
Debezium normalizer are required before any of those can change. Segment
publication, durable quarantine, control offsets, and spool TTL are parent
work. This crate does not mark SP04 complete.

Pinned dependencies are the `cdc-transaction` lock with the package name
changed to `cdc-spool`. No crate was added. `cargo update` and
`cargo generate-lockfile` were not run.

| crate | version | checksum |
|---|---|---|
| serde | 1.0.229 | `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` |
| serde_core | 1.0.229 | `67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48` |
| serde_derive | 1.0.229 | `e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348` |
| serde_json | 1.0.151 | `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14` |

SHA-256 is the FIPS 180-4 function copied from `tools/cdc-transaction/src/sha256.rs`
(`eac61aa8d9d0eebe935be0d438b6877b40845656601c4c5a8180edd97963f037`).
Empty input is `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
`abc` is `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.
`rust-version` is 1.94.0. Edition 2024. Safe Rust only.

## Feed

UTF-8 JSONL, one object per line, LF separated. A CR is malformed. The line
body, excluding the LF, is at most 1 MiB (1048576 bytes). The byte that
would make it longer is not parsed and is not stored. Duplicate object keys,
including decoded `\u` escapes, are malformed. Unknown fields are malformed.
There is no per-row LSN. A row LSN is never a commit position.

The stream is one `bind`, one `begin`, that transaction's `data` records,
and one `end` or one `rollback`. A barrier, a second source, a second
lineage, a second epoch, or a second transaction id is `single_stream` or
`active_txn_exceeded`. The active-transaction cap is 1.

`bind`, `begin`, `data`, `end`, and `rollback` use the `cdc-transaction-v1`
field rules. Ordinals are checked as integers before they are stored.
`0..=32767` is accepted. `32768` and `65535`, and any other integer outside
that range, are `ordinal_range` and are not stored. A non-integer ordinal
is malformed. `null` and `unavailable` stay distinct. Decimal trailing zeros
stay significant. Text, bytes, bool, and int strings are preserved.

The event hash is lowercase hex SHA-256 of the compact preimage, fields in
this order: `data_collection_sequence`, `global_txn_sequence`, `key`, `op`,
`payload`, `table`, `txn_id`, `unavailable_ordinals`. Cell objects are
`ordinal`, `type`, then `value` when a value exists. The sample preimage
for sequence 3 of `inv17` hashes to
`4864cde4bd37438f0fa3c69808c0adb5ddd244d2450987ad69299330b2b8e8ee`.

`end` checks the declared count, the checked sum of `per_collection_counts`,
and the exact set of global and per-collection sequences. The commit LSN
and the source commit timestamp come only from `end`. The position is
`epoch << 64 | commit_lsn`, the same decimal rule as `cdc-transaction-v1`.
Epoch `1` and LSN `16/B6E40` encode as `18446744168199581248`.

An end that is still short of `count` is `end_too_early`. Stored chunks
stay pending. The transaction is not sealed. A hole, a zero, or a sequence
above the declared count refuses completion. The same global sequence with
the same canonical body is idempotent and does not change the count. A
different body, or the same collection sequence on one table under a new
global sequence, is `duplicate_conflict`.

`rollback` writes `ROLLED_BACK` and does not write `SEAL` or
`manifest.json`. A second rollback of that closed transaction is
idempotent. Rollback after a seal is `rollback_after_complete`.

## Directory

`create_new` creates the caller-supplied transaction directory. The parent
must be a real directory, not a symlink. If the path already exists as a
directory, file, symlink, sealed transaction, or foreign pending directory,
the call is `path_exists` and nothing there is overwritten, deleted, or
adopted. Chunk names are only `chunk-000001` through the file cap, six
digits, not `chunk-000000`. Recovery opens those exact child names. It does
not follow symlinks and it does not join a name that contains a slash.

| name | role |
|---|---|
| `chunk-NNNNNN.partial` | bytes written, not yet a durable chunk |
| `chunk-NNNNNN` | chunk after flush, `sync_all`, and rename |
| `chunks.idx` | one durable line per synced chunk, after the chunk sync |
| `progress.json` | durable progress, not a commit and not applied |
| `manifest.json` | committed manifest bytes |
| `SEAL` | 64 lowercase hex SHA-256 of the manifest bytes, plus LF |
| `ROLLED_BACK` | exact body `rolled_back\n` |

A chunk line is one canonical preimage plus LF. The chunk hash is SHA-256
of those exact file bytes. As each chunk is sealed, its hex hash is
appended to the chunk-set input. `chunk_set_sha256` is SHA-256 of the
ordered lines `{chunk_sha256}\n`.

A chunk is durable only after `write`, `flush`, `sync_all`, a length and
hash reread, rename to the final name, and a parent-directory `sync_all`.
The index line is appended and synced after that. `SEAL` is created only
after every indexed chunk, `manifest.json`, and the transaction directory
have been synced. `Drop` does not seal, roll back, or delete.

`progress.json` records `durable_chunks`, `durable_events`, and
`durable_bytes` from synced index lines. `decoded_events` is the count
accepted in the process and can be ahead of `durable_events` until the
open chunk is synced. `applied` is null. `ack_allowed`, `target_applied`,
and `cross_table_consistent` are false. Recovery does not treat progress
as a commit.

## Recovery

`recover` yields a committed chunk iterator only when all of these hold:

- `SEAL` is a regular non-symlink file of 64 lowercase hex digits plus LF.
- `manifest.json` is a regular non-symlink file whose SHA-256 matches `SEAL`.
- The manifest parses, names this contract, and keeps every attestation false and `applied` null.
- `chunks.idx` lists the same chunk names, lengths, event counts, and hashes.
- Each chunk path is the fixed child name, a regular non-symlink file, and its length and SHA-256 match.

The iterator then rereads one chunk at a time, at most the sealed byte
length, and refuses `output_mismatch` if a line is not the canonical
preimage or the file hash changes. It does not write.

Missing `end`, a missing chunk, a truncated chunk, a checksum mismatch, or
a symlink is not a committed transaction and is not ACK. A torn `SEAL`
or a torn commit write is `unknown`. It is not `rolled_back`, and files
are not deleted. `rolled_back` is reported only when `ROLLED_BACK` has the
exact body, is a regular file, and `SEAL` is absent.

Exact replay of a sealed directory recomputes event hashes from the feed
and compares them to the sealed chunks. A match writes nothing. A payload
difference is `payload_conflict` and writes nothing. Replay of an
unsealed directory is `not_committed`.

A kill during chunk `sync_all`, during `end`, or before `SEAL` leaves
`incomplete` or `unknown`. It does not report `rolled_back` and it does
not report a committed iterator.

## Caps

Product caps are the maximum. A caller may lower them. Raising one is
`limit_raised`. The disk cap is a byte counter of chunk bytes, a 128-byte
index allowance per chunk, and the progress, manifest, seal, and rollback
bytes this process writes. It is not an operating-system quota. The excess
line is not written. Already written owned files stay.

| cap | value | refusal |
|---|---|---|
| active transactions | 1 | `active_txn_exceeded` |
| data events | 500000 | `event_cap` |
| events in one chunk | 8192 | chunk is sealed first |
| bytes in one chunk | 4194304 | chunk is sealed first |
| files in the directory | 96 | `file_cap` |
| disk bytes | 268435456 | `disk_quota` |
| JSONL line body | 1048576 | `line_too_long` |
| collections | 256 | `collection_invalid` |
| cells in one key or payload | 4096 | `key_invalid` or `payload_invalid` |
| ordinal | 32767 | `ordinal_range` |

The in-memory assembler bound of 200000 events does not apply here. A
transaction of 500000 events can seal. Event 500001 cannot. Payloads are
not retained. The process keeps the open chunk buffer (at most 4 MiB),
sequence bitsets, and 32 bytes of hash per admitted sequence. An optional
Linux `VmHWM` reading is a measurement only when `/proc/self/status`
exists. A missing reading is not a claimed resident size.

## CLI

`cdc-spool spool <dir>` reads JSONL on stdin and creates `<dir>`.
`cdc-spool recover <dir>` verifies and writes one status object.
`cdc-spool replay <dir>` reads JSONL on stdin and compares it to the seal.
No other arguments. Exit 0 is a sealed transaction or a durable rollback.
Exit 3 is EOF with the transaction incomplete and no refusal. Exit 2 is a
refusal. Exit 1 is usage or I/O.

`CDC_SPOOL_CHECKPOINT=before_chunk_sync|before_end|before_final_seal` and
`CDC_SPOOL_MARK` are a fault hook. When unset, the process does not pause.
When set, it writes the mark after the named write and before the matching
sync or seal, then waits. Tests use it so a parent can `SIGKILL` the child.
It is not a durability feature.

## Sample

Selected tables `public.lines` and `public.orders`. Three data events for
`inv17`: order `17` upsert, line `1` delete, line `1` upsert with a null
cell and an unavailable cell. End count 3, commit LSN `16/B6E40`, timestamp
`2026-04-15T08:30:00.123456Z`, position `18446744168199581248`. The sealed
manifest records those bindings, chunk hashes, `first_sequence` 1,
`last_sequence` 3, and every attestation false.

The 500000-event fixture is two tables of small int cells, sequences
`1..=500000`, odd `public.orders` and even `public.lines`, produced by the
Rust test into a file. The expected preimage and hash are the ordered
formula and the test's own SHA-256, checked against the empty and `abc`
literals. The product hash function is not the sole expected digest.
