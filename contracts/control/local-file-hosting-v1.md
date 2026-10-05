# Local file hosting

Contract `oo.control.local-file-hosting/v1`. Library `oo_control` (`file_hosting`).

This slice stores one upload session on an [`object_store::ObjectStore`]. `FileHost::open_local` binds a deployment directory to `LocalFileSystem` and turns fsync on. `FileHost::new` accepts any `Arc<dyn ObjectStore>` as the seam for a later approved backend. Without the local deployment lock, mutating calls return `Unsupported` before they allocate. This crate enables object_store feature `fs` only. It does not enable `aws`, `gcp`, `azure`, or `http`. It does not talk to MinIO, and a passing local test is not an S3 result.

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

`open_download(&AdmissionContext, &ObjectKey, chunk_budget, deadline) -> ObjectDownload`

`ObjectDownload::next_chunk() -> Result<Option<Vec<u8>>, FileHostError>`

`chunk_budget` is a `usize`. `deadline` is `Option<Instant>`. `ObjectDownload::manifest` returns the manifest snapshot from open. `is_complete` is true only after a verified end. `cancel` refuses later chunks.

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

`LocalFileSystem` multipart state is an in-memory handle and a `dest#N` staging file. That handle does not survive a dropped `FileHost`. Parts are therefore ordinary objects. `complete` hashes each part by ranged reads of at most 8192 bytes. That limit is fixed in the library before each `get_opts` range is built. A response larger than 8192 bytes is `Store` and does not publish. `put_multipart_opts` runs only after those streamed reads already hash and measure to the expected sha256 and size. The publish pass uses the same 8192-byte ranges and writes each chunk into the multipart handle. The local complete renames the staging file onto the object key. A checksum or size miss does not start that upload, so the object key is not created.

The 8192-byte bound is the host's internal read and multipart write, and the maximum `open_download` chunk. Bytes the caller passes to `put_part` may already occupy the caller's memory. `read_object` still returns one buffer of the stored object size filled from those chunks. `open_download` does not build that buffer: each `next_chunk` returns at most the chunk budget, and the host does not keep the returned chunk. A caller that retains only the current chunk holds one budget-sized buffer. This contract does not claim a process limit outside those API buffers.

## Idempotency

The idempotency digest is SHA-256 over the domain `oo.file-hosting/v1 idempotency` and a trailing NUL, then each of these strings followed by a NUL: project, principal, expected sha256 as lowercase hex, expected size in decimal, display name, content type or empty. Grant evidence is not an input. A later `begin` with the same project, principal, and configuration returns the same session, including when the grant string changed. The stored grant stays the one from the first `begin`. A different display name, content type, size, or digest is a different key and a different session. It does not replace the first session's parts.

If the idempotency pointer already names a session whose stored project, principal, digest, size, display name, content type, idempotency key, or object key does not match the request, `begin` returns `Conflict` and writes nothing.

`put_part` of the same bytes for the same part number returns the same receipt. Different bytes return `Conflict` and leave the stored part in place. There is no `PutMode::Update`.

## Completion, abort, and quota

`complete` on a session that already has a manifest returns that manifest after the project and principal match. Otherwise it lists the five-digit part objects and uses each object's stored length. A gap, or a byte count below the expected size, is `Incomplete`. The session and parts stay. A byte count above the expected size is `Size`. The stored digest mismatch is `Checksum`. Both `Size` and `Checksum` delete only this session's part objects, quota reservation, idempotency pointer, and session record. They do not write the object key or the manifest.

On a match, `complete` publishes through `put_multipart_opts`, re-hashes the published object, then creates the manifest, then deletes the part objects. The session, idempotency pointer, quota reservation, manifest, and object key remain. If the published bytes do not re-hash to the same digest and size, the object key just written is deleted and the manifest is not written (`Corrupt`). A second `complete` is the stored manifest. The deployment lock stays held from the part listing through that manifest write, so a concurrent `put_part` or `abort` cannot replace a part after it was hashed and before it is published. One of the concurrent `complete` calls writes the manifest. The other returns that same manifest.

`abort` of an open session deletes those same owned keys. `abort` of a completed session is `Closed` and deletes nothing. `abort` or `put_part` of a session that is already gone is `NotFound`. A second `abort` is `NotFound`.

Project quota is the sum of `meta/quota/<project>/<session>` bodies whose names are 32 lowercase hex characters. Other names under that prefix are ignored and are not deleted. `begin` refuses with `Quota` when the new expected size would pass `max_project_bytes`. A resumed `begin` does not reserve twice. A completed object keeps its reservation. Abort and the checksum/size cleanup release it.

On `open_local`, that check and the reservation write are one critical section. The same section covers `put_part`, `complete`, and `abort` for every `FileHost` on that deployment root, including other processes. The lock is `std::fs::File::try_lock` on a file the host creates beside the deployment directory. Its name is the directory's file name plus `.oo-file-hosting.lock`. Callers do not choose that path. An in-process gate orders tasks in the same process, because the file lock is what orders other processes. The wait is 30 seconds of retries. The runtime thread is not the thread that sleeps on the lock. Timeout or a lock-file error returns `Unavailable` and writes nothing. A poisoned `std::sync::Mutex` in this wait is recovered with the inner guard and the wait continues. Poison is not turned into `Unavailable` and does not by itself refuse the call. A crash drops the lock with the process. The lock is advisory: a raw store client can still write keys. Two machines that do not share this directory do not share the lock.

Document delta, this revision: the previous sentence said a poisoned wait returns `Unavailable`. `lock_mutex` recovers with `into_inner` and continues. This paragraph matches that recovery. No production fault injection was added for it.

`FileHost::new` has no deployment lock. `begin`, `put_part`, `complete`, and `abort` on that host return `Unsupported` before any store write. There is no unlocked quota fallback. A later S3 backend has to bring its own conditional atomic quota before those calls can succeed. This local lock is not that backend.

Cleanup deletes only keys this session recorded or that match its five-digit part names. A name such as `staging/<session>/part/notes` is left in place. A file outside the store prefix is left in place. Empty directories are left in place. A `dest#N` file left by a crash during `put_multipart` is not reaped here.

## Manifest and lineage

The manifest contract string is `oo.control.local-file-hosting/v1`. Fields are the object key, session, project, principal, grant evidence, display name, content type, object sha256, size, part count, idempotency key, and lineage digest. Session and manifest JSON are canonical: sorted keys, no extra whitespace, version 1. Decode re-encodes and requires the same bytes. The lineage digest is checked again on decode.

The lineage digest is SHA-256 over the domain `oo.file-hosting/v1 lineage` and a trailing NUL, then each of these strings followed by a NUL: session, project, principal, grant evidence, object key, object sha256 as lowercase hex, size in decimal, part count in decimal, idempotency key, display name, content type or empty.

`read_object` loads the manifest, reads the object bytes into one buffer of the object size, and returns them only when their sha256 and length match the manifest. A mismatch is `Corrupt`.

## Download

`open_download` is a local pull of an object this host has already published. It is not an HTTP download, not a public URL, and not a credential. The caller passes the same trusted `AdmissionContext` and the platform `ObjectKey` already returned for that object. The call does not take a filesystem path. `ObjectKey::parse` still accepts only `uploads/<session>/<sha256>/original`. Anything else is `NotFound`, and the text is not copied into the error.

A chunk budget of `0` or above 8192 is `Limit` before any store read. A missing manifest is `NotFound`. A manifest that decodes but names a different object key is `NotFound`. A different project or principal is `Denied` and deletes nothing. A decoded manifest whose size is zero is `Corrupt`. A stored object length that is not the manifest size, including a zero-length file, is `Corrupt`. `begin` still rejects an expected size of zero, so this host does not publish an empty object. `next_chunk` does not return an empty vector. End of a verified pull is `Ok(None)`, not an empty chunk.

The manifest value from `manifest()` is the snapshot taken at open: contract, object key, session, project, principal, digest, size, part count, idempotency key, and lineage. The reader keeps those fields stable. It does not replace them if the stored manifest changes.

Each object-body read is one `get_opts` with `GetRange::Bounded`. The range is chosen before the call. Its length is the smaller of the chunk budget and the bytes remaining in the manifest size, and it is never above 8192. An empty buffer, a buffer longer than the budget, or a buffer whose length is not the requested range is `Store`. The host does not read the next range until the caller calls `next_chunk` again, and it does not retain the returned vector. The running sha256 is updated from that chunk only.

`Ok(None)` and `is_complete() == true` happen only when every object byte has been returned to the caller and two digests equal the snapshot. The first is the sha256 of the bytes `next_chunk` returned. The second is a separate end-of-object pass over the stored object. That pass reads `GetRange::Bounded` ranges of at most 8192 bytes, hashes each range, and drops the range before the next one. It does not build a vector of the object size. Before that pass, and again after it, the stored manifest bytes must still equal the open snapshot and the stored length must still equal the snapshot size. A chunk is not completion. `cancel`, a deadline that has passed, `Drop`, or stopping before that end leaves `is_complete` false. After a verified end, further `next_chunk` calls return `Ok(None)` without reading again. `cancel` after that end does not erase the verification and is not revocation.

On `open_local`, the end-of-object pass also safe-stats the object file. The stat uses the file's own metadata and does not follow a symlink. A symlink, or a path that leaves the deployment directory, is refused. The local identity is the device, inode, modified time, and length. Those four values must be the same before and after the rehash. The local backend reports an empty object `version`. That empty field is not an immutability token and is not treated as one. A generic store from `FileHost::new` has no deployment path, so it has no local stat. Its proof is the same two digests, the manifest bytes, and the length. When that store reports an etag or a version, those values must also be unchanged across the rehash. A missing etag and a missing version are not replaced with a made-up version. The proof does not write a quota row.

The check is the object at that clean end. It is not a promise that the file stays unchanged afterward. A write that lands after `is_complete` is already true is a race on the shared file. It does not clear `is_complete`, it is not revocation, and a later `next_chunk` still returns `Ok(None)` without reading again.

The deadline is the caller's `Instant`. `next_chunk` checks it, and checks cancel, before every `get_opts` and every `bytes` await and again after that await returns. The same checks cover every range of the end-of-object rehash, the manifest reloads around it, and the local stat. `None` does not disable the cancel check. The deadline is not a TTL. If the check fails after `get_opts` returns, `bytes` is not started. If it fails after one range returns, the next range is not started. An await that has already been polled is not cancelled. A local filesystem read that has started runs until that call returns, and this host does not claim to preempt it. `cancel` and a passed deadline are `Stopped`, whose display text is `download stopped before the object was verified`. `Drop` of the reader sets that cancel flag and releases the store handle and hash state the reader owns. `Drop` of an in-flight `next_chunk` sets the same flag so the next check stops the pull. Neither one aborts the filesystem call already in flight, and neither one is a `Read` cancel or a credential.

A changed manifest, a stored-object hash that is not the snapshot, a returned-bytes hash that is not the snapshot, a stored length that no longer matches, a local stat identity that changes during the rehash, or a capped manifest body that is empty or above 4096 bytes is `Corrupt` once the pull has started, and the pull does not become complete. Flipping a byte that was already returned is `Corrupt`: the running hash of the returned chunks can still match the snapshot while the stored object does not, and both have to match. The manifest reload is a bounded range of at most 4096 bytes, not an object-sized buffer.

`open_download` does not take the deployment lock and does not change quota. `FileHost::new` can pull an object that is already stored. Mutating calls on that host are still `Unsupported`.

`read_object` is unchanged. Its caller still receives one buffer of the object size.

## Authorization

`put_part`, `complete`, `abort`, `read_manifest`, `read_object`, and `open_download` compare the caller project and principal with the stored session or manifest. A difference is `Denied` and deletes nothing. Grant evidence is still stored and is not checked. The context is trusted input from a future admission step. This module does not authenticate it. `Denied` on a key the caller already holds does confirm that this key is occupied. Path secrecy is not the control, and this slice does not claim the two errors are indistinguishable.

A key that is not `uploads/<session>/<sha256>/original`, a session id that is not 32 lowercase hex characters, a missing manifest, or a digest that is not the one stored on that session's manifest is `NotFound`. The wrong-digest case is `NotFound` for the owner and for every other context, so a guessed digest is not confirmed. The same bytes uploaded by another project are a new session and a new object key. That response does not include the other project's id or session id.

`FileHostError` display text is a fixed sentence. Store errors are not chained, because the local backend's `NotFound` and `AlreadyExists` text contains a filesystem path. `FileHost`'s debug text omits the store.

## What stays open

SP05-W01 is not closed. The ledger is unchanged. SP05-W06 is not closed. The local pull above is not a download proxy, not retention, and not a public download.

Still open:

- an S3 backend contract, including the 5 MiB minimum part size, conditional quota, and any MinIO run
- reaping `dest#N` leftovers and a crash between deleting owned keys
- HTTP, an auth proxy, current-authorization checks beyond the trusted project and principal tuple, revocation, and TTL
- the caller's `read_object` buffer of the object size
- MIME sniffing, formula-safe CSV or Excel output, and retention delete
- presigned URLs
- quota across hosts that do not share one deployment directory
- the rest of the REQ-SRC-05 state machine after `stored`
- checking the grant evidence instead of storing it
- customer deployment, production release, and a reaper
