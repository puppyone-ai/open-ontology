# Local file hosting

Contract `oo.control.local-file-hosting/v1`. Library `oo_control` (`file_hosting`).

This slice stores one upload session on an [`object_store::ObjectStore`]. `FileHost::open_local` binds a deployment directory to `LocalFileSystem` and turns fsync on. `FileHost::new` accepts any `Arc<dyn ObjectStore>`, which is the seam for a later approved S3 backend. This crate enables object_store feature `fs` only. It does not enable `aws`, `gcp`, `azure`, or `http`. It does not talk to MinIO, and a passing local test is not an S3 result.

The caller passes an `AdmissionContext` (project, principal, grant evidence) that a future control admission step has already accepted. This module does not authenticate that context, does not check a signature on the grant, and does not speak HTTP. An unguessable key is not authorization. Upload, complete, abort, and read methods do not take a caller filesystem path. The deployment directory is an operator argument to `open_local` only.

Plan, apply, secrets, and RBAC are not implemented in `oo-control`.

## Pin

Direct registry pins, exact versions, default features off:

| Crate | Version | Role |
|---|---|---|
| object_store | 0.14.2 | store trait and local backend, feature `fs` only |
| sha2 | 0.10.9 | content, idempotency, and lineage digests, feature `std` |
| getrandom | 0.2.17 | session id bytes, feature `std` |
| futures-util | 0.3.34 | store list and get streams, feature `std` |
| tokio | 1.53.1 | dev-dependency only (`macros`, `rt-multi-thread`) |

`object_store` 0.14.2 was published on 2026-09-15. The crates.io checksum is `f1796bc93603f78c5760a69f2d58badc9618d22adade0a95385bb2adbae4eb94`. Its manifest `rust-version` is 1.85. The manifest license is MIT OR Apache-2.0. This project relies on Apache-2.0. The product `rust-version` remains 1.94.0. sha2 0.10.9, getrandom 0.2.17, futures-util 0.3.34, and tokio 1.53.1 were already in the workspace lock; this slice does not bump them.

The `fs` feature also links the crates object_store declares for that feature (including `nix` on unix and `walkdir`). Those versions are whatever `cargo fetch` resolves from the existing lock plus object_store's own requirements. The evidence directory records the lock delta. No AWS SDK crate is a dependency of this feature.

## Call

`begin(&AdmissionContext, &UploadConfig) -> UploadSession`

`put_part(&AdmissionContext, &SessionId, part_number, bytes) -> PartReceipt`

`complete(&AdmissionContext, &SessionId) -> ObjectManifest`

`abort(&AdmissionContext, &SessionId) -> ()`

`read_manifest(&AdmissionContext, &ObjectKey) -> ObjectManifest`

`read_object(&AdmissionContext, &ObjectKey) -> StoredObject`

`idempotency_key(&AdmissionContext, &UploadConfig) -> IdempotencyKey` is pure. It does not touch the store. Host limits are not part of the key. `SessionId::parse` and `ObjectKey::parse` accept only values this host already issues. A bad value is `NotFound` and the text is not copied into the error.

`UploadConfig` carries the display name, the expected sha256 of the whole object, the expected size, and an optional content type. The display name is metadata. It is never a path segment.

## Identifiers and bounds

Project and principal: 1 to 128 bytes, a leading lowercase letter, then lowercase letters, digits, `_`, or `-`. Grant evidence: 1 to 512 bytes, no NUL and no control characters. The grant is stored and is not checked. Display name: 1 to 255 bytes, no NUL, no controls, no `/`, no `\`. Content type, when present: 3 to 128 bytes, one `/` not at either end, ASCII alphanumeric plus `+`, `-`, `.`, `_`, and no `..`. It is stored as given. Bytes are not sniffed.

`HostLimits::standard` is a 5_000_000_000 byte object, a 5_000_000_000 byte part, 10_000 parts, and a 50_000_000_000 byte project quota. `HostLimits::try_new` rejects zero, an object ceiling above 5_000_000_000, a part larger than the object, a part count above 10_000, or a zero project quota. `begin` rejects an expected size of zero or above the host object ceiling. `put_part` rejects an empty part, a part above the part ceiling, a part number outside `1..=max_part_count`, and a running sum that would pass the expected object size. The local store does not enforce the S3 5 MiB minimum part size.

A refused `begin` or `put_part` does not create or replace an object.

## Keys

Session ids are 16 random bytes, lowercase hex. Object keys are `uploads/<session>/<sha256>/original`, where `<sha256>` is the expected object digest. Other durable keys, all under the store prefix:

| Key | Body |
|---|---|
| `meta/session/<session>` | canonical session JSON, created once |
| `meta/manifest/<session>` | canonical manifest JSON, created once at complete |
| `meta/idempotency/<project>/<principal>/<idem>` | the 32-hex session id |
| `meta/quota/<project>/<session>` | the expected size, decimal, no leading zeros |
| `staging/<session>/part/<nnnnn>` | raw part bytes, `nnnnn` is the part number in five digits |

`LocalFileSystem` multipart state is an in-memory handle and a `dest#N` staging file. That handle does not survive a dropped `FileHost`. Parts are therefore ordinary objects. `complete` calls `put_multipart_opts` only after those objects already hash and measure to the expected sha256 and size. The local complete renames the staging file onto the object key. A checksum or size miss does not start that upload, so the object key is not created.

## Idempotency

The idempotency digest is SHA-256 over the domain `oo.file-hosting/v1 idempotency` and a trailing NUL, then each of these strings followed by a NUL: project, principal, expected sha256 as lowercase hex, expected size in decimal, display name, content type or empty. Grant evidence is not an input. A later `begin` with the same project, principal, and configuration returns the same session, including when the grant string changed. The stored grant stays the one from the first `begin`. A different display name, content type, size, or digest is a different key and a different session. It does not replace the first session's parts.

If the idempotency pointer already names a session whose stored project, principal, digest, size, display name, content type, idempotency key, or object key does not match the request, `begin` returns `Conflict` and writes nothing.

`put_part` of the same bytes for the same part number returns the same receipt. Different bytes return `Conflict` and leave the stored part in place. There is no `PutMode::Update`.

## Completion, abort, and quota

`complete` on a session that already has a manifest returns that manifest after the project and principal match. Otherwise it loads the five-digit part objects. A gap, or a byte count below the expected size, is `Incomplete`. The session and parts stay. A byte count above the expected size is `Size`. The stored digest mismatch is `Checksum`. Both `Size` and `Checksum` delete only this session's part objects, quota reservation, idempotency pointer, and session record. They do not write the object key or the manifest.

On a match, `complete` publishes through `put_multipart_opts`, re-hashes the published object, then creates the manifest, then deletes the part objects. The session, idempotency pointer, quota reservation, manifest, and object key remain. If the published bytes do not re-hash to the same digest and size, the object key just written is deleted and the manifest is not written (`Corrupt`). A second `complete` is the stored manifest.

`abort` of an open session deletes those same owned keys. `abort` of a completed session is `Closed` and deletes nothing. `abort` or `put_part` of a session that is already gone is `NotFound`. A second `abort` is `NotFound`.

Project quota is the sum of `meta/quota/<project>/<session>` bodies whose names are 32 lowercase hex characters. Other names under that prefix are ignored and are not deleted. `begin` refuses with `Quota` when the new expected size would pass `max_project_bytes`. A resumed `begin` does not reserve twice. A completed object keeps its reservation. Abort and the checksum/size cleanup release it. Two concurrent `begin` calls can both pass the check before either reservation is visible. That race is open.

Cleanup deletes only keys this session recorded or that match its five-digit part names. A name such as `staging/<session>/part/notes` is left in place. A file outside the store prefix is left in place. Empty directories are left in place. A `dest#N` file left by a crash during `put_multipart` is not reaped here.

## Manifest and lineage

The manifest contract string is `oo.control.local-file-hosting/v1`. Fields are the object key, session, project, principal, grant evidence, display name, content type, object sha256, size, part count, idempotency key, and lineage digest. Session and manifest JSON are canonical: sorted keys, no extra whitespace, version 1. Decode re-encodes and requires the same bytes. The lineage digest is checked again on decode.

The lineage digest is SHA-256 over the domain `oo.file-hosting/v1 lineage` and a trailing NUL, then each of these strings followed by a NUL: session, project, principal, grant evidence, object key, object sha256 as lowercase hex, size in decimal, part count in decimal, idempotency key, display name, content type or empty.

`read_object` loads the manifest, reads the object bytes, and returns them only when their sha256 and length match the manifest. A mismatch is `Corrupt`.

## Authorization

`put_part`, `complete`, `abort`, `read_manifest`, and `read_object` compare the caller project and principal with the stored session or manifest. A difference is `Denied` and deletes nothing. `Denied` on a key the caller already holds does confirm that this key is occupied. Path secrecy is not the control, and this slice does not claim the two errors are indistinguishable.

A key that is not `uploads/<session>/<sha256>/original`, a session id that is not 32 lowercase hex characters, a missing manifest, or a digest that is not the one stored on that session's manifest is `NotFound`. The wrong-digest case is `NotFound` for the owner and for every other context, so a guessed digest is not confirmed. The same bytes uploaded by another project are a new session and a new object key. That response does not include the other project's id or session id.

`FileHostError` display text is a fixed sentence. Store errors are not chained, because the local backend's `NotFound` and `AlreadyExists` text contains a filesystem path. `FileHost`'s debug text omits the store.

## What stays open

SP05-W01 is not closed. The ledger is unchanged. SP05-W06 is not started.

Still open:

- an S3 backend contract, including the 5 MiB minimum part size, and any MinIO run
- reaping `dest#N` leftovers and a crash between deleting owned keys
- download, TTL, an auth proxy, and HTTP
- MIME sniffing, formula-safe CSV or Excel output, and retention delete
- presigned URLs
- a linearizable project quota
- the rest of the REQ-SRC-05 state machine after `stored`
- checking the grant evidence instead of storing it
