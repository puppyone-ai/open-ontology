# cdc-quarantine-v1

Durable bounded quarantine for one normalized control event. The event is a schema change or a TRUNCATE. This crate stores the event. It does not apply it.

This is not a Debezium router, a target applier, an offset store, or a fenced writer. `source_position` is caller-declared decimal text. Nothing in this crate parses an LSN, opens PostgreSQL, or reads a spool.

`ack_eligible` and `ack_allowed` stay false. `schema_applied`, `target_truncated`, and `cross_table_consistent` stay false. `target_applied` stays null. `debezium_router` and `upstream_commit_verified` stay false. `durable_quarantined` turns on only `local_prerequisite`, and only after the receipt below exists. Upstream ACK stays false until a real fenced writer and control offset are integrated. This unit does not do that integration.

## Control object

One JSON object. Exactly these keys. Duplicate keys are malformed, including keys that differ only by `\u` escapes. Unknown keys are malformed. The payload is a JSON string. Its UTF-8 bytes are opaque. They are not SQL and they are not base64.

| Key | Rule |
| --- | --- |
| `kind` | `control` |
| `source` | `^[a-z][a-z0-9_]{0,62}$` |
| `lineage` | 64 lowercase hex digits |
| `epoch` | JSON integer `0..=4294967295`, no fraction, no exponent, no leading zero |
| `event_id` | `^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$` |
| `sequence` | JSON integer `>= 1` |
| `collection` | `schema.table`. Schema uses the source charset. Table is `^[A-Za-z_][A-Za-z0-9_]{0,127}$`. One dot. |
| `reason` | `schema_changed` or `truncate` |
| `payload` | Non-empty JSON string, at most `max_payload_bytes` after decode |
| `payload_sha256` | Lowercase hex SHA-256 of the decoded payload bytes |
| `source_position` | Canonical decimal, 1 to 38 digits, no leading zero except `0` |

Identity, and the event directory name, is the SHA-256 of this UTF-8 text, with no extra space:

```text
{"epoch":EPOCH,"event_id":"EVENT_ID","lineage":"LINEAGE","source":"SOURCE"}
```

Those four fields cannot contain characters that would change the JSON spelling, so the preimage is exact.

A digest that does not match the payload is `digest_mismatch`. An empty or over-cap payload is `payload_too_large`. Both responses still name `attempt_collection` and set `paused` true. Neither response creates a directory.

## Limits

Product caps, which an environment variable may only lower:

| Cap | Value | Variable |
| --- | --- | --- |
| Payload bytes | 65536 | `CDC_QUARANTINE_MAX_PAYLOAD_BYTES` |
| Events | 1024 | `CDC_QUARANTINE_MAX_EVENTS` |
| Disk byte counter | 67108864 | `CDC_QUARANTINE_MAX_DISK_BYTES` |

A value above the product cap is `limit_raised` and writes nothing. `0` is a legal lowered cap. The JSON line body is at most 1048576 bytes. A paused-collection list is at most 256 names. The disk figure is the sum of regular-file lengths this process has accounted. It is not an operating-system quota.

The checkpoint variables are test hooks, not durability:

- `CDC_QUARANTINE_CHECKPOINT` is `before_data_fsync`, `before_manifest`, `before_rename`, or `before_parent_fsync`
- `CDC_QUARANTINE_MARK` is a path outside the store

When the stage matches, the process creates that mark with `create_new`, writes `waiting\n`, fsyncs the mark, and sleeps until it is killed. The mark is not a store record.

## Layout

The caller passes an absolute or relative path whose components contain no `..` and no symlink prefix. The parent directory must already exist. The store root is created with `create_dir` only when it is absent. An existing empty directory, or a directory that contains only alien names, is `foreign_directory` and is not adopted or deleted. A root that already contains `events/` or `STORE` and is not a complete quarantine is `incomplete` and is not repaired.

```text
ROOT/STORE
ROOT/events/<64 lowercase hex>/payload
ROOT/events/<64 lowercase hex>/manifest.json
ROOT/events/<64 lowercase hex>/RECEIPT
```

`STORE` is four lines: `cdc-quarantine-v1`, source, lineage, epoch. The first event writes it. A later event with a different source, lineage, or epoch is `binding_conflict` and writes nothing.

`manifest.json` is one strict JSON object plus a trailing LF. Reserializing the parsed object must reproduce those bytes. Field order is the order in this list. Every attestation inside the file is false or null, including `local_prerequisite`. The file records the paused collections known at seal time. It is not rewritten when a later event arrives.

`paused_collections` is sorted, unique, contains this event's collection, and every name is a collection. Recovery unions that list across every durable manifest, and unions any collection it can still read from an incomplete manifest. A missing hint on torn bytes sets `unknown_incomplete` and still pauses.

`RECEIPT` is three lines, written only after the payload, the manifest, and `STORE` have been renamed into place and the event directory, `events/`, and the root have been fsynced:

```text
durable_quarantined
<payload sha256>
<manifest sha256>
```

The receipt is then fsynced and the parent directories are fsynced again. `durable_quarantined` in the status is true only when that receipt matches. The manifest file itself never says `local_prerequisite: true`.

Same identity and the same payload bytes, sequence, collection, reason, digest, and position is a replay. Replay checks the stored bytes and does not rewrite them. Same identity and any difference in those fields is `payload_conflict` and writes nothing.

## Durability order

For a new event the process:

1. Refuses the attempt before creating anything when the record cap or disk cap would be exceeded.
2. Creates the event directory and `payload.partial`, writes the payload, and flushes.
3. Fsyncs `payload.partial` and reads it back.
4. Writes and fsyncs `manifest.json.partial`. On the first event, writes and fsyncs `STORE.partial`.
5. Renames payload, manifest, and `STORE` onto their final names only when the destination is absent.
6. Fsyncs the event directory, `events/`, and the root.
7. Creates `RECEIPT`, fsyncs it, and fsyncs those directories again.

A crash before step 7 leaves the tree incomplete. Recovery does not seal, delete, or rewrite it. A second `put` or `replay` against an incomplete or foreign tree refuses and does not finish the receipt.

SIGKILL of the process is a real checkpoint, not a proof about torn file bytes. The kernel may flush a page after the signal. Tests assert which names exist, and that recovery stays incomplete, paused, and not ACK-eligible.

## Status

`put`, `replay`, and `recover` print one JSON object of kind `quarantine_status` on stdout. The payload bytes are not echoed.

| Exit | Meaning |
| --- | --- |
| 0 | Durable result, including an exact replay |
| 3 | Incomplete tree and no fault code |
| 2 | Refused attempt, including conflict, quota, digest, and incomplete-on-write |
| 1 | Usage, I/O, or not found |

`local_prerequisite` is true only when `durable_quarantined` is true, `incomplete` is false, and `fault` is null. `paused` is true when a recognized control attempt was named or when the tree holds a durable or incomplete control event. A quota refusal does not persist the refused event. The response still names `attempt_collection`.

`recover` reads the tree. It does not create files. A clean tree of one or more receipted events is durable and paused. Two collections stay paused together. Their names are sorted. No target truncate or schema apply runs in any command.

## CLI

```text
cdc-quarantine put|recover|replay <dir>
```

`put` and `replay` read one JSON object from stdin, stopping at LF. CR is malformed. Bytes beyond the line cap are discarded up to one extra cap and are not stored. A second object on the same line is malformed. `recover` reads no stdin.

Symlink paths are refused. Opens of store files use `O_NOFOLLOW` (`0x20000` on Linux, `0x0100` on macOS) through `OpenOptionsExt::custom_flags`. The store directory uid must match the process uid from `/proc/self/status`. This crate does not call `remove_file`, `remove_dir`, or `canonicalize`, and it does not follow a symlink to read a payload.
