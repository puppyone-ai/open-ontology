# Gateway audit WAL

Contract `oo.gateway.audit-wal/v1`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a371`.

This is the frozen durability seam for one local spool in front of the existing `DedicatedSession` audit hook. It does not accept SP06-W06, SP02-W06, public untrusted SQL, runsc, a feedback receiver, the gateway parent, or a release.

## Hook

`DiskWal` implements `oo_gateway::query_executor::AuditAdmission`. `admit` and `complete` read the existing getters only. This crate does not construct `Admission` or `CompletionRecord` and it does not run a second query executor.

`admit` appends one admission frame and `fsync`s it before it returns. The existing executor calls `admit` before `BEGIN` and before any result row. `complete` appends a separate completion frame and `fsync`s that frame on its own. A completion `fsync` failure leaves the admission in place and does not write a succeeded completion.

Guard refusal does not call the hook. The spool gains a refusal frame only when the operator calls `record_not_admitted`. That method takes a project and a role. It has no SQL argument. The reason token is `explicit_not_admitted`.

## Identity and bounds

Each frame is keyed by `(spool_id, request_id, event_seq)`.

| Event | `event_seq` | Kind |
|---|---|---|
| Admission | 1 | `admission` |
| Completion for that same request | 2 | `completion` |
| Explicit not-admitted | 1 | `refusal` |

`spool_id` is generated once when the directory is created and then read back. A later open does not mint a new spool id. `request_id` is generated inside `admit` or `record_not_admitted`. The executor does not supply a query id.

Admission metadata is the getter set: original SHA-256, executed SHA-256, fingerprint, role, project, policy id, backend pid, and backend start. Completion repeats those fields and adds status, truncation, row count, byte count, rollback, socket drop, stop class, and SQLSTATE. A completion whose immutable fields differ from its admission is not written.

Field bytes are bounded and charset-checked. SHA-256 fields are 64 lowercase hex characters. Backend start is decimal digits. Role and project are short identifiers. The payload has no SQL text, no secret, and no result body. A frame is at most 16 KiB of payload plus a fixed header and a SHA-256 checksum.

## Disk

Segments are `seg-NNNNNNNN.wal`. Creating a directory, a lock file, a spool id, or a segment is followed by an `fsync` of the new file and of the parent directory. The writer holds `std::fs::File::try_lock` on `writer.lock` for the life of `DiskWal`. Rust 1.94 has that method. A second opener gets `Busy` and does not write.

`WalConfig.max_bytes` is the spool quota. A frame that would pass the quota is not written. `admit` then returns `ExecError::AuditUnavailable`. Write, `fsync`, and `ENOSPC` do the same, and the partial append is truncated back before the error returns. No query transaction starts. There is no unbounded queue.

## Crash and replay

A reopened spool returns every checksum-valid frame. A short tail that does not contain a full frame is a torn write: earlier frames stay, and the tail is not a record. A full frame whose checksum fails, including a frame that is followed by a later valid frame, fails the open. The later frame is not returned. A sealed segment whose footer hash does not match also fails the open.

Replay state for one request:

| Frames on disk | State |
|---|---|
| Admission only | `admitted` |
| Admission plus a completion frame | the completion status word |
| Refusal only | `not_admitted` |
| Completion without admission | corrupt |

`succeeded` is only the status word inside a durable completion frame. Replaying an admission does not promote it.

## Collector checkpoint and GC

`wal-send` posts sealed frames to the audit collector over rustls mTLS. It checkpoints a frame only after the HTTPS response parses as `oo.audit.ingest/v1` and the spool id, request id, event sequence, and payload hash equal the frame that was sent. A caller file or flag named `ack` is not read. Collector errors and timeouts leave the frame in the spool. Retries are bounded: 3 attempts, 50 ms then 100 ms then 200 ms.

`wal-inspect gc` deletes a segment only when it is sealed, it is not the active segment, every frame in it has a checkpoint row with the same payload hash, and the sealed file hash equals the hash stored in the checkpoint. The checkpoint is `fsync`ed before GC. An unacked or partially acked segment stays. A checkpoint row whose segment hash disagrees with the file fails closed and deletes nothing. The checkpoint hash for a segment does not change.

## Binaries

`wal-inspect replay` prints frame metadata and the replay state. `wal-inspect gc` is the GC above. `wal-send` is the mTLS sender. None of them is an authenticated public HTTP endpoint. Private keys are paths inside a mode `0600` config file, not argv values, environment values, or log lines.

## Out of scope

Feedback intake, control-plane admin roles, a query-gateway credential for the control database, public untrusted SQL, and runsc are not this contract.
