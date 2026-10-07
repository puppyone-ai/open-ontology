# FileHost completed-object deletion

Superseded in part by `FILEHOST-COMPLETION-ADDENDUM.md`. Where the two disagree, the completion addendum wins. In particular, quota is not released before the bytes, sidecars, parts, and platform staging are absent; `stop_after_quota_release` is not a caller flag; an open abort can prove part absence; and `--pause-after quota_released` is rejected. The procedure below that releases quota first is the historical freeze, not the implementation.

Contract `oo.control.local-file-hosting/v1` addendum, file `FILEHOST-DELETE-CONTRACT.md`.
This file freezes the upstream deletion interface, the receipt, and the reaper reconciliation before any source change.
It does not replace `contracts/control/local-file-hosting-v1.md`. That approved text still describes `abort` of a completed session as `Closed`.
`abort` stays that way. Completed-object deletion is a different call.

This addendum does not close SP05-W06, AT-FILE-05, or AT-SEC-03. `parent_complete` stays false.

## Where the types live

`crates/oo-control/src/lib.rs` is not edited. The new calls are methods on the already exported `FileHost`.
Their arguments and results use only types `oo_control` already exports, plus `bool`, `u8`, `Option`, and `[u8; 32]`.
No new crate-root type is required.

```text
delete_completed(
    ctx,
    object_key,
    object_digest,
    size,
    lineage_digest,
    stop_after_quota_release,
) -> Result<Option<ObjectManifest>, FileHostError>

observe_completed_absence(
    ctx,
    object_key,
    object_digest,
    size,
    lineage_digest,
) -> Result<bool, FileHostError>

completed_quota_release_count(ctx, object_key) -> Result<u8, FileHostError>
```

`FileHost::new` has no deployment lock. All three calls return `Unsupported` before any store read or write.
`FileHost::open_local` holds the existing deployment lock for the whole call. The lock file stays the host's `<dirname>.oo-file-hosting.lock`. Callers do not choose it and this work does not replace it.

There is no filesystem-path argument, no caller session id, and no caller quota path. The session id is the 32 hex characters inside the platform `ObjectKey` `uploads/<session>/<sha256>/original`. A key that is not that shape cannot be constructed. A digest argument that is not the digest embedded in that key is `NotFound` and writes nothing.

## Authorization precondition

The caller passes the current project, principal, and grant, plus the exact object key, object sha256, size, and lineage digest taken from a manifest this host already returned.

When the manifest is still present:

| Mismatch | Result | Writes |
|---|---|---|
| project or principal | `Denied` | none |
| grant evidence bytes | `Denied` | none |
| object key, object sha256, size, or lineage | `NotFound` | none |

Grant evidence is compared on this delete path. `begin`, `put_part`, `complete`, `abort`, `read_manifest`, `read_object`, and `open_download` stay as approved: they do not check the grant. A wrong grant on delete does not confirm a different project's object. `Denied` still means this key is occupied by a manifest whose project and principal matched and whose grant did not. Path secrecy is not claimed.

A session record that is present must match the manifest's project, principal, grant, object key, digest, and size. A mismatch is `Conflict` and writes nothing.
The quota object, when present, must be the decimal size with no leading zeros. Any other body is `Conflict` and writes nothing.

A guessed id, a `sessions/<id>/outputs/` string, or `../` is not a delete request. `ObjectKey::parse` already rejects those shapes.

## What deletion may remove

Only these keys, and only after the precondition matches:

| Key | Role |
|---|---|
| `meta/quota/<project>/<session>` | the one reservation, body equal to the manifest size |
| `staging/<session>/part/<nnnnn>` | five-digit part names only |
| `uploads/<session>/<sha256>/original` | the object body |
| `meta/manifest/<session>` | the manifest |
| `meta/session/<session>` | the session record |
| `meta/idempotency/<project>/<principal>/<idem>` | the pointer stored on that manifest |

`<project>`, `<principal>`, `<session>`, and `<idem>` come from the manifest, not from a caller path. A name such as `staging/<session>/part/notes` is left in place. A file outside the store prefix is left in place. Empty directories are left in place. A `dest#N` file left by `put_multipart` is not reaped.

## Symlink and non-file

Before any tombstone is written, each of those keys that exists is checked with `symlink_metadata` under the canonical deployment root. The check does not follow a symlink. A symlink, a directory, or any non-regular file is `Corrupt`. Nothing is unlinked, including the symlink itself, and no quota key is removed.

The same check applies to each key before it is deleted on a later phase. A symlink discovered after the quota phase returns `Corrupt`, does not unlink, and does not take a second quota release.

## Tombstone

One object, `meta/tombstone/<session>`, at most 4096 bytes. It is not a manifest and it is not a quota row. The body is this line record and nothing else:

```text
oo.control.local-file-hosting/v1 tombstone
phase intent|quota_released|verified
object_key <platform key>
object_digest <64 lowercase hex>
size <decimal>
lineage_digest <64 lowercase hex>
project <project id>
principal <principal id>
grant_hex <lowercase hex of the grant bytes>
display_name <display name>
content_type <content type, or - when absent>
part_count <decimal>
idempotency_key <64 lowercase hex>
quota_key meta/quota/<project>/<session>
quota_bytes <decimal, equal to size>
release_count 0|1
```

`content_type` is `-` only for a missing content type. A real content type contains one `/`, so it is not `-`.
`quota_key` must be exactly the key derived from the manifest project and session. `release_count` is only `0` or `1`. A body that does not decode, is larger than 4096 bytes, or disagrees with itself is `Corrupt`.

The tombstone is overwritten in place only forward: absent to `intent`, `intent` to `quota_released`, `quota_released` to `verified`. A later call does not write an earlier phase and does not raise `release_count`.

Phases:

1. `intent`, `release_count 0`. Written only when no tombstone exists and the precondition, quota body, and symlink checks have passed. The manifest fields needed to rebuild `ObjectManifest` are copied here before any delete.
2. Delete exactly `quota_key`. `NotFound` on that delete is success of the delete call. No other quota key is listed or removed. Then the tombstone becomes `quota_released` with `release_count 1`.
3. If `stop_after_quota_release` is true and the phase is `quota_released`, return `Ok(None)`. The object, manifest, session, and idempotency pointer are still present. This is not completion.
4. Delete the five-digit parts, the object, the manifest, the session, and the idempotency pointer, in that order. Each delete is idempotent on `NotFound`. Then observe absence. Only when that observation is `Ok(true)` does the tombstone become `verified` and the call return `Ok(Some(manifest))`. The manifest value is the snapshot saved in the tombstone, not a new read of a deleted object.

A crash after the quota key is gone and before `quota_released` is durable leaves phase `intent` and `release_count 0`. Restart deletes that same missing key again and then sets `release_count 1`. The bytes were freed once, because only that key contributed them. Restart does not free a second reservation.

A crash during the object phase leaves `quota_released` and `release_count 1`. Restart continues the object deletes and does not change `release_count`.

A verified tombstone plus a passing absence check returns `Ok(Some(manifest))` again. `release_count` stays `1`. The stop flag does not rewind a verified tombstone. If a deleted key is present again, the call returns `Conflict` and does not release quota.

`Ok(None)` means the quota phase is durable and absence is not verified. `Ok(Some(_))` means absence was verified and the single release is recorded. Any `Err` is not completion.

## Quota, once

Project quota stays the sum of `meta/quota/<project>/<session>` bodies. Release is the removal of the one key named in the tombstone. There is no counter decrement and no second key. `completed_quota_release_count(ctx, object_key)` reads the tombstone under the lock. It does not take a separate digest. The object key already embeds the sha256, and the tombstone's stored key must equal the caller's key.

| State | Result |
|---|---|
| no tombstone | `Ok(0)` |
| tombstone project, principal, or grant does not match | `Denied` |
| tombstone object key does not match | `NotFound` |
| match | `Ok(0)` or `Ok(1)` |

The value is never greater than 1. A missing tombstone does not prove that an object exists or that it was deleted. Digest, size, and lineage are checked by `delete_completed` and `observe_completed_absence`, not by this count.

After a verified delete, `begin` with the same project, principal, and upload configuration does not return the deleted session. The idempotency pointer is gone, so the host allocates a new session when the released bytes fit. A second `begin` that would pass `max_project_bytes` is still `Quota`. A repeated delete does not make that second `begin` succeed.

## Public absence

`observe_completed_absence` takes the deployment lock, writes nothing, and does not unlink.

`Ok(true)` is the only absence proof. It requires all of these:

- a tombstone in phase `verified` whose key, digest, size, lineage, project, principal, and grant match the caller
- the object key, the manifest key, and every five-digit part key are store `NotFound`
- none of those paths is a symlink or a non-file
- no store error occurred while checking them

`Ok(false)` means the precondition matches and at least the object or the manifest is still a regular object. That is not failure of the store. It is presence.

| Condition | Result |
|---|---|
| store error, including a list or stat failure that is not `NotFound` | `Err(Store)` |
| symlink or non-file on the object, the manifest, or a five-digit part | `Err(Corrupt)` |
| manifest present, project or principal or grant mismatch | `Err(Denied)` for grant or tuple mismatch as in the table above |
| manifest present, hash or size or lineage or key mismatch | `Err(NotFound)` |
| no tombstone and no manifest | `Err(NotFound)` |
| tombstone says `verified` but a key is still present | `Err(Corrupt)` |

`read_object` or `open_download` returning `NotFound` is not this proof. A store outage is not absence. `NotFound` from those readers is not rewritten into `Ok(true)`.

`dest#N` files are outside this observation. Their presence does not make `Ok(true)` false, and this contract does not claim they were reaped.

## Readers already in progress

`open_download` and `next_chunk` do not take the deployment lock and do not change quota. Delete does not cancel them and does not preempt a filesystem read that has already started.

A pull that already returned `Ok(None)` with `is_complete == true` stays complete. Later `next_chunk` calls return `Ok(None)` without reading. That is the approved end-of-object race. It is not revocation and it is not a credential.

A pull that has not finished verification can observe the delete on its next range. `get_opts` failure, including a missing object, is `Store` and leaves `is_complete` false. A missing object or manifest during the end check is `Corrupt` and leaves `is_complete` false. Neither result is an absence proof. Bytes already returned are not recalled.

## Reaper

`tools/artifact-retention` calls `delete_completed` for an expired published registration. It does not unlink store files and it does not edit quota files.

The deployment owner is an explicit operator UUID, not a hardcoded task id. The grammar is 8-4-4-4-12 lowercase hex with hyphens. `Deployment::create(root, limits, operator)` refuses any other string, a symlink root, a final path component that is not `operator`, and `..`. `owner.txt` records that UUID and the `HostLimits`. `Deployment::open` reads `owner.txt`, checks the same grammar, and requires the directory name to match. An a311 directory still opens when its name and `owner.txt` carry that UUID. A different valid UUID is the same rule, not a new component.

Caps are unchanged. Uploaded source is 180 days. Workspace snapshot is 24 hours and only when `explicit_workspace` is true. Session output, transcript, quarantine, and eval stay on their existing caps. The download permit cap stays 300 seconds. `gateway_url_revocation` stays `not_implemented`. `bearer_url` stays `none`.

The expired set is still the inventory rows whose `expiry_unix_ms <= now`. The reaper does not scan the store for unregistered keys.

For a published row the order is:

1. Fsync the pre-delete audit and the checkpoint `pre_delete`. The audit bound stays 64 records. Past that bound the call returns `Limit` and does not delete.
2. Observe through `open_download` as today. Hash, size, lineage, symlink, directory, or store error: do not call `delete_completed`. A store error is `read_error_not_absence`, not absence.
3. Call `delete_completed` with `stop_after_quota_release = true`. On `Ok(None)`, fsync checkpoint `quota_released`.
4. If the caller asked to pause at `quota_released`, return `paused` with `deletion_success` false, `deleted` false, and `absence_verified` false. The CLI writes `control/waiting` and polls `control/continue`. Exit 75. The process has not printed success.
5. Call `delete_completed` with `stop_after_quota_release = false`.
6. Call `observe_completed_absence`. Success for that row requires `Ok(Some(_))` from the second delete, `Ok(true)` from the observation, and `completed_quota_release_count == 1`. Status is `deleted`. `deleted` and `absence_verified` are true.

`deletion_success` is true only when every expired row reached that published success. An open row still uses `abort` and stays `aborted_parts_unverified` with success false, so a batch that contains one does not set `deletion_success`. An idle reap, with nothing expired, has `deletion_success` false and `idle` true.

A checkpoint of `pre_delete`, `metadata_api_calling`, `metadata_api_done`, or `quota_released` makes the next reap `restarted_from_partial`. The checkpoint file remains a cursor. Its `absence_verified` and `deletion_success` lines stay `false`. It is not a success receipt. Restart calls the host delete again. The host tombstone decides the phase. Restart does not report success until step 6 passes, and it does not move `release_count` past 1.

A second reap of a verified row calls the same host path. The tombstone stays `verified`, the count stays 1, and absence is checked again. A foreign project row that is not expired is not passed to delete. An unregistered object is not passed to delete.

The CLI is `artifact-retention reap --root <operator-uuid-dir> [--pause-after pre_delete|metadata_api|data_deleting|data_absent|none]`. `quota_released` is rejected. There is no clock flag and no bearer URL. Exit 0 means idle or `deletion_success`. Exit 2 means a reap ran and deletion is not verified. Exit 75 is the pause. Exit 1 is a bad root, a corrupt record, or a bad argument. Exit 0 is not parent acceptance.

## Still unsupported

`required_integration` returns these names. It no longer returns `filehost_completed_object_delete`, `filehost_quota_release_on_completed_delete`, `filehost_part_absence_observation`, or `filehost_dest_staging_reap`:

| Seam | Why it remains |
|---|---|
| `filehost_object_version` | The manifest has no version. An empty local version is not recorded. |
| `filehost_admission_role_and_policy` | The host delete checks project, principal, and grant. Role and policy stay on the inventory row. |
| `gateway_authenticated_url_revocation` | No HTTP transport and no authenticated download URL. |
| `formula_safe_csv_excel` | No CSV or Excel writer. |
| `sessions_outputs_prefix` | FileHost keys stay `uploads/<session>/<sha256>/original`. |

`parent_complete` is false on every report, including a report whose `deletion_success` is true.
